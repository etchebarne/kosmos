import { useReducer, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { OverlayScrollbarsComponent } from "overlayscrollbars-react";
import { Dialog } from "../shared/Dialog";
import { PillButton } from "../shared/PillButton";
import { ScrollArea } from "../shared/ScrollArea";
import { useWorkspaceStore } from "../../store/workspace.store";

interface RemoteDialogProps {
  open: boolean;
  onClose: () => void;
  distro: string;
}

interface DirEntry {
  name: string;
  is_dir: boolean;
}

interface BrowserState {
  cwd: string;
  entries: DirEntry[];
  loading: boolean;
  error: string | null;
  status: string | null;
  connecting: boolean;
}

type BrowserAction =
  | { type: "SET_CWD"; cwd: string }
  | { type: "FETCH_START" }
  | { type: "FETCH_SUCCESS"; entries: DirEntry[] }
  | { type: "FETCH_ERROR"; error: string }
  | { type: "RESET" }
  | { type: "CONNECT_START" }
  | { type: "CONNECT_STATUS"; status: string }
  | { type: "CONNECT_FAIL"; status: string };

const initialState: BrowserState = {
  cwd: "/",
  entries: [],
  loading: true,
  error: null,
  status: null,
  connecting: false,
};

function browserReducer(state: BrowserState, action: BrowserAction): BrowserState {
  switch (action.type) {
    case "SET_CWD":
      return { ...state, cwd: action.cwd };
    case "FETCH_START":
      return { ...state, loading: true, error: null };
    case "FETCH_SUCCESS":
      return { ...state, entries: action.entries, loading: false };
    case "FETCH_ERROR":
      return { ...state, error: action.error, entries: [], loading: false };
    case "RESET":
      return { ...state, status: null, connecting: false, error: null };
    case "CONNECT_START":
      return { ...state, connecting: true, status: "Deploying agent..." };
    case "CONNECT_STATUS":
      return { ...state, status: action.status };
    case "CONNECT_FAIL":
      return { ...state, status: action.status, connecting: false };
  }
}

export function RemoteDialog({ open, onClose, distro }: RemoteDialogProps) {
  const openWorkspace = useWorkspaceStore((s) => s.openWorkspace);
  const [state, dispatch] = useReducer(browserReducer, initialState);
  const { cwd, entries, loading, error, status, connecting } = state;

  // Resolve home dir on open
  useEffect(() => {
    if (!open) return;
    dispatch({ type: "RESET" });
    invoke<string>("wsl_resolve_home", { distro })
      .then((home) => dispatch({ type: "SET_CWD", cwd: home }))
      .catch(() => dispatch({ type: "SET_CWD", cwd: "/" }));
  }, [open, distro]);

  // Fetch directory listing when cwd changes
  useEffect(() => {
    if (!open || !cwd) return;
    dispatch({ type: "FETCH_START" });
    invoke<DirEntry[]>("wsl_list_dir", { distro, path: cwd })
      .then((result) => dispatch({ type: "FETCH_SUCCESS", entries: result }))
      .catch((e) => dispatch({ type: "FETCH_ERROR", error: String(e) }));
  }, [open, distro, cwd]);

  const navigate = useCallback(
    (name: string) => {
      const next = cwd === "/" ? `/${name}` : `${cwd}/${name}`;
      dispatch({ type: "SET_CWD", cwd: next });
    },
    [cwd],
  );

  const navigateUp = useCallback(() => {
    if (cwd === "/") return;
    const normalized = cwd.endsWith("/") ? cwd.slice(0, -1) : cwd;
    const parent = normalized.substring(0, normalized.lastIndexOf("/")) || "/";
    dispatch({ type: "SET_CWD", cwd: parent });
  }, [cwd]);

  const navigateBreadcrumb = useCallback(
    (index: number) => {
      if (index === 0) {
        dispatch({ type: "SET_CWD", cwd: "/" });
        return;
      }
      const segments = cwd.split("/").filter(Boolean);
      const path = "/" + segments.slice(0, index).join("/");
      dispatch({ type: "SET_CWD", cwd: path });
    },
    [cwd],
  );

  const handleConnect = useCallback(async () => {
    dispatch({ type: "CONNECT_START" });

    try {
      try {
        await invoke("deploy_agent_wsl", { distro });
      } catch (e) {
        dispatch({ type: "CONNECT_FAIL", status: `Agent deploy failed: ${e}` });
        return;
      }

      dispatch({ type: "CONNECT_STATUS", status: "Connecting..." });

      await invoke("remote_connect", {
        workspacePath: `wsl://${distro}${cwd}`,
        connection: { type: "wsl", distro },
      });

      await openWorkspace(`wsl://${distro}${cwd}`, {
        type: "wsl",
        distro,
      });

      onClose();
    } catch (e) {
      dispatch({ type: "CONNECT_FAIL", status: `Connection failed: ${e}` });
    }
  }, [cwd, distro, openWorkspace, onClose]);

  const segments = cwd.split("/").filter(Boolean);
  const dirs = entries.filter((e) => e.is_dir);

  return (
    <Dialog open={open} onClose={onClose} title={`Connect to WSL: ${distro}`}>
      <div className="flex flex-col" style={{ height: 380 }}>
        {/* Breadcrumb path bar */}
        <OverlayScrollbarsComponent
          className="border-b border-[var(--color-border-primary)] bg-[var(--color-bg-input)] min-h-[36px] shrink-0"
          options={{
            scrollbars: {
              autoHide: "scroll",
              autoHideDelay: 800,
              theme: "os-theme-custom",
            },
            overflow: { x: "scroll", y: "hidden" },
          }}
        >
          <div className="flex items-center gap-0.5 px-3 py-2 w-max">
            <button
              className="text-xs px-1 py-0.5 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-hover)] shrink-0 rounded"
              onClick={() => navigateBreadcrumb(0)}
            >
              /
            </button>
            {segments.map((seg, i) => (
              <span key={i} className="flex items-center shrink-0">
                <span className="text-[10px] text-[var(--color-text-muted)]">/</span>
                <button
                  className={`text-xs px-1 py-0.5 hover:bg-[var(--color-bg-hover)] rounded ${
                    i === segments.length - 1
                      ? "text-[var(--color-text-primary)] font-medium"
                      : "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
                  }`}
                  onClick={() => navigateBreadcrumb(i + 1)}
                >
                  {seg}
                </button>
              </span>
            ))}
          </div>
        </OverlayScrollbarsComponent>

        {/* Directory listing */}
        <ScrollArea className="flex-1">
          {loading ? (
            <div className="flex items-center justify-center h-full">
              <span className="text-xs text-[var(--color-text-muted)]">Loading...</span>
            </div>
          ) : error ? (
            <div className="flex items-center justify-center h-full px-4">
              <span className="text-xs text-[var(--color-status-red)]">{error}</span>
            </div>
          ) : (
            <div className="flex flex-col">
              {/* Go up */}
              {cwd !== "/" && (
                <button
                  className="flex items-center gap-2.5 mx-2 px-3 py-1.5 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text-primary)] text-left rounded-md"
                  onClick={navigateUp}
                >
                  <span className="w-4 text-center text-[var(--color-text-muted)]">..</span>
                  <span>..</span>
                </button>
              )}
              {dirs.map((entry) => (
                <button
                  key={entry.name}
                  className="flex items-center gap-2.5 mx-2 px-3 py-1.5 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text-primary)] text-left rounded-md"
                  onClick={() => navigate(entry.name)}
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 16 16"
                    fill="currentColor"
                    className="shrink-0 text-[var(--color-text-muted)]"
                  >
                    <path d="M1.5 2A1.5 1.5 0 000 3.5v9A1.5 1.5 0 001.5 14h13a1.5 1.5 0 001.5-1.5V5a1.5 1.5 0 00-1.5-1.5H7.707l-1.354-1.354A.5.5 0 006 2H1.5z" />
                  </svg>
                  <span>{entry.name}</span>
                </button>
              ))}
              {dirs.length === 0 && cwd === "/" && (
                <div className="px-3 py-4 text-xs text-[var(--color-text-muted)] text-center">
                  No subdirectories
                </div>
              )}
            </div>
          )}
        </ScrollArea>

        {/* Footer */}
        <div className="flex items-center justify-between gap-3 px-3 py-2.5 border-t border-[var(--color-border-primary)] shrink-0">
          <span className="text-[11px] text-[var(--color-text-muted)] truncate flex-1 min-w-0">
            {status ?? cwd}
          </span>
          <div className="flex gap-2 shrink-0">
            <PillButton variant="ghost" size="sm" onClick={onClose} disabled={connecting}>
              Cancel
            </PillButton>
            <PillButton variant="accent" size="sm" onClick={handleConnect} disabled={connecting}>
              {connecting ? "Connecting..." : "Open"}
            </PillButton>
          </div>
        </div>
      </div>
    </Dialog>
  );
}
