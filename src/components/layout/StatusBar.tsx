import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { GitBranch } from "@phosphor-icons/react";
import { useWorkspaceStore } from "../../store/workspace.store";
import { useLspStore, resolveServerLanguage, type ServerStatus } from "../../store/lsp.store";
import { useLayoutStore } from "../../store/layout.store";
import { findLeaf } from "../../lib/paneTree";
import { languageIdFromExt } from "../../lib/extToLang";
import { getFileExtension } from "../../lib/pathUtils";
import { Dialog } from "../shared/Dialog";
import { PillButton } from "../shared/PillButton";

const STATUS_LABELS: Record<ServerStatus, string> = {
  running: "",
  starting: "starting...",
  error: "error",
  unavailable: "not installed",
  installing: "installing...",
  stopped: "stopped",
};

function filePathToServerLang(filePath: string): string | null {
  const ext = getFileExtension(filePath);
  if (!ext) return null;
  const langId = languageIdFromExt(ext);
  if (!langId) return null;
  return resolveServerLanguage(langId);
}

export function StatusBar() {
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const activeIndex = useWorkspaceStore((s) => s.activeIndex);
  const activePath = activeIndex !== null ? (workspaces[activeIndex]?.path ?? null) : null;
  const progress = useLspStore((s) => (activePath ? s.indexProgress[activePath] : undefined));
  const servers = useLspStore((s) => (activePath ? s.servers[activePath] : undefined));
  const installServer = useLspStore((s) => s.installServer);
  const layout = useLayoutStore((s) => s.layout);
  const activePaneId = useLayoutStore((s) => s.activePaneId);

  const [branch, setBranch] = useState<string | null>(null);
  const [installDialog, setInstallDialog] = useState<{
    serverName: string;
    languageId: string;
  } | null>(null);

  useEffect(() => {
    if (!activePath) {
      setBranch(null);
      return;
    }

    let cancelled = false;

    async function fetchBranch() {
      try {
        const result = await invoke<string | null>("get_git_branch", {
          path: activePath,
        });
        if (!cancelled) setBranch(result);
      } catch {
        if (!cancelled) setBranch(null);
      }
    }

    fetchBranch();
    const interval = setInterval(fetchBranch, 5000);

    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [activePath]);

  const activeProgress = progress?.length ? progress : null;

  let focusedServerLang: string | null = null;
  if (activePaneId) {
    const leaf = findLeaf(layout, activePaneId);
    if (leaf?.activeTabId) {
      const activeTab = leaf.tabs.find((t) => t.id === leaf.activeTabId);
      if (activeTab?.type === "editor" && activeTab.metadata?.filePath) {
        focusedServerLang = filePathToServerLang(activeTab.metadata.filePath as string);
      }
    }
  }

  const focusedServer = focusedServerLang && servers ? servers[focusedServerLang] : null;
  const showLsp = focusedServer && focusedServer.status !== "stopped";

  const handleLspClick = (serverName: string, languageId: string, status: ServerStatus) => {
    if (status === "unavailable") setInstallDialog({ serverName, languageId });
  };

  const handleInstall = async () => {
    if (!installDialog || !activePath) return;
    setInstallDialog(null);
    await installServer(activePath, installDialog.serverName);
  };

  return (
    <div className="flex items-center gap-3 h-6 min-h-6 px-4 bg-[var(--color-bg-page)] pill-depth border border-[var(--color-border-primary)] text-[var(--color-text-secondary)] text-[11px] rounded-full overflow-hidden">
      <div className="flex items-center gap-1">
        <GitBranch size={12} />
        <span>{branch ?? "Not a git repo"}</span>
      </div>
      {activeProgress &&
        activeProgress.length > 0 &&
        (() => {
          const item = activeProgress[0];
          const label = item.message ?? item.title;
          const pct = item.percentage != null ? ` ${item.percentage}%` : "";
          return (
            <div className="flex items-center gap-1.5 text-[var(--color-text-muted)] animate-pulse">
              <span className="max-w-[200px] truncate">
                {label}
                {pct}
              </span>
            </div>
          );
        })()}
      <div className="flex-1" />
      <div className="flex items-center gap-3">
        {showLsp && (
          <>
            <button
              className={`flex items-center ${focusedServer.status === "unavailable" ? "cursor-pointer hover:text-[var(--color-text-primary)]" : "cursor-default"} ${focusedServer.status === "installing" ? "animate-pulse" : ""}`}
              title={focusedServer.errorMessage ?? focusedServer.serverName}
              onClick={() =>
                handleLspClick(
                  focusedServer.serverName,
                  focusedServer.languageId,
                  focusedServer.status,
                )
              }
            >
              <span className="text-[var(--color-text-secondary)]">
                {focusedServer.serverName}
                {STATUS_LABELS[focusedServer.status] && (
                  <span className="text-[var(--color-text-muted)]">
                    {" "}
                    ({STATUS_LABELS[focusedServer.status]})
                  </span>
                )}
              </span>
            </button>
            <Dialog
              open={installDialog !== null}
              onClose={() => setInstallDialog(null)}
              title={`Install ${installDialog?.serverName ?? ""}?`}
            >
              <div className="p-4 flex flex-col gap-4">
                <p className="text-xs text-[var(--color-text-secondary)]">
                  <span className="text-[var(--color-text-primary)] font-medium">
                    {installDialog?.serverName}
                  </span>{" "}
                  was not found on your system. Would you like to install it?
                </p>
                <div className="flex justify-end gap-2">
                  <PillButton variant="ghost" size="sm" onClick={() => setInstallDialog(null)}>
                    Cancel
                  </PillButton>
                  <PillButton variant="accent" size="sm" onClick={handleInstall}>
                    Install
                  </PillButton>
                </div>
              </div>
            </Dialog>
          </>
        )}
      </div>
    </div>
  );
}
