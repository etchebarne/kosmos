import { useEffect, useCallback, useState, useMemo } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ArrowsClockwise, ArrowsInLineVertical, FilePlus, FolderPlus } from "@phosphor-icons/react";
import { useActiveWorkspace, useIsWorkspaceActive } from "../../contexts/WorkspaceContext";
import { FileTreeNode } from "./FileTreeNode";
import { useFileTreeSelection } from "./fileTreeStores";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { StateView } from "../../components/shared/StateView";
import { Tooltip } from "../../components/shared/Tooltip";
import { useGitStatus } from "../../hooks/useGitStatus";
import { GitFileTreeContext, buildGitColorLookup } from "./gitFileTreeContext";
import { getCached, getOrFetch, invalidate } from "./fileTreeCache";
import type { TabContentProps } from "../types";

export interface DirEntry {
  name: string;
  path: string;
  isDir: boolean;
  extension: string | null;
}

export function FileTreeTab({ tab: _tab, paneId }: TabContentProps) {
  const activeWorkspace = useActiveWorkspace();
  const isActive = useIsWorkspaceActive();
  const workspacePath = activeWorkspace?.path ?? null;

  const initialEntries = workspacePath ? getCached(workspacePath) : null;
  const [entries, setEntries] = useState<DirEntry[]>(initialEntries ?? []);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!initialEntries);
  const [externalDrop, setExternalDrop] = useState<{ dirPath: string; rect: DOMRect } | null>(null);
  const { status: gitStatus } = useGitStatus(workspacePath, isActive);

  const getGitColor = useMemo(() => {
    if (!activeWorkspace || !gitStatus?.isRepo) return () => null;
    return buildGitColorLookup(gitStatus.changes, activeWorkspace.path);
  }, [activeWorkspace, gitStatus]);

  const loadRoot = useCallback(async () => {
    if (!workspacePath) return;
    if (getCached(workspacePath)) {
      setEntries(getCached(workspacePath)!);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await getOrFetch(workspacePath);
      setEntries(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    loadRoot();
  }, [loadRoot]);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest("[data-file-tree]")) {
        useFileTreeSelection.getState().clear();
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  // Only the active workspace watches — the watcher is a singleton per process.
  useEffect(() => {
    if (!activeWorkspace || !isActive) return;
    invoke("watch_workspace", { path: activeWorkspace.path }).catch((e) =>
      console.warn("Failed to start file watcher:", e),
    );
    return () => {
      invoke("unwatch_workspace", { path: activeWorkspace.path });
    };
  }, [activeWorkspace, isActive]);

  useEffect(() => {
    if (!workspacePath || !isActive) return;

    const findTarget = (physX: number, physY: number) => {
      const dpr = window.devicePixelRatio || 1;
      const x = physX / dpr;
      const y = physY / dpr;
      const elements = document.elementsFromPoint(x, y);
      if (!elements.some((el) => (el as HTMLElement).dataset?.fileTree !== undefined)) {
        return null;
      }
      for (const el of elements) {
        const dirPath = (el as HTMLElement).dataset?.dirPath;
        if (dirPath) {
          return { dirPath, rect: (el as HTMLElement).getBoundingClientRect() };
        }
      }
      return null;
    };

    let unlisten: (() => void) | null = null;
    let cancelled = false;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          setExternalDrop(findTarget(p.position.x, p.position.y));
        } else if (p.type === "leave") {
          setExternalDrop(null);
        } else if (p.type === "drop") {
          const target = findTarget(p.position.x, p.position.y);
          setExternalDrop(null);
          if (!target) return;
          for (const sourcePath of p.paths) {
            invoke("copy_entry", { source: sourcePath, destDir: target.dirPath })
              .then(() => {
                window.dispatchEvent(
                  new CustomEvent("file-tree-move", {
                    detail: {
                      sourcePath: `\0__external__/${sourcePath}`,
                      destDir: target.dirPath,
                      fileName: "",
                    },
                  }),
                );
              })
              .catch((err: unknown) => console.error("Failed to copy dropped file:", err));
          }
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
      setExternalDrop(null);
    };
  }, [workspacePath, isActive]);

  // One listener fans events out via CustomEvents; per-node listeners caused
  // a read_dir thundering herd that starved the runtime.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let pending = new Set<string>();

    const unlisten = listen<string[]>("file-tree-changed", (event) => {
      for (const dir of event.payload) {
        invalidate(dir);
        pending.add(dir);
      }
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        for (const dir of pending) {
          window.dispatchEvent(new CustomEvent("file-tree-refresh", { detail: { dir } }));
        }
        pending = new Set();
      }, 300);
    });

    return () => {
      if (timer) clearTimeout(timer);
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleNewFile = useCallback(() => {
    if (!workspacePath) return;
    window.dispatchEvent(
      new CustomEvent("file-tree-create", {
        detail: { dir: workspacePath, type: "file" },
      }),
    );
  }, [workspacePath]);

  const handleNewFolder = useCallback(() => {
    if (!workspacePath) return;
    window.dispatchEvent(
      new CustomEvent("file-tree-create", {
        detail: { dir: workspacePath, type: "dir" },
      }),
    );
  }, [workspacePath]);

  const handleRefresh = useCallback(() => {
    if (!workspacePath) return;
    const dirs = new Set<string>([workspacePath]);
    document.querySelectorAll<HTMLElement>("[data-file-tree] [data-dir-path]").forEach((el) => {
      const dirPath = el.dataset.dirPath;
      if (dirPath) dirs.add(dirPath);
    });
    for (const dir of dirs) {
      invalidate(dir);
      window.dispatchEvent(new CustomEvent("file-tree-refresh", { detail: { dir } }));
    }
  }, [workspacePath]);

  const handleCollapseAll = useCallback(() => {
    window.dispatchEvent(new CustomEvent("file-tree-collapse-all"));
  }, []);

  if (!activeWorkspace) {
    return <StateView message="No workspace open" />;
  }

  if (loading && entries.length === 0) {
    return <StateView message="Loading..." variant="secondary" />;
  }

  if (error) {
    return <StateView message={error} variant="error" />;
  }

  const rootEntry: DirEntry = {
    name: activeWorkspace.name,
    path: activeWorkspace.path,
    isDir: true,
    extension: null,
  };

  const toolbarBtn =
    "p-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-elevated)] transition-colors cursor-pointer rounded";

  const rootActions = (
    <>
      <Tooltip content="New File">
        <button className={toolbarBtn} onClick={handleNewFile}>
          <FilePlus size={14} />
        </button>
      </Tooltip>
      <Tooltip content="New Folder">
        <button className={toolbarBtn} onClick={handleNewFolder}>
          <FolderPlus size={14} />
        </button>
      </Tooltip>
      <Tooltip content="Refresh Explorer">
        <button className={toolbarBtn} onClick={handleRefresh}>
          <ArrowsClockwise size={14} />
        </button>
      </Tooltip>
      <Tooltip content="Collapse Folders">
        <button className={toolbarBtn} onClick={handleCollapseAll}>
          <ArrowsInLineVertical size={14} />
        </button>
      </Tooltip>
    </>
  );

  return (
    <GitFileTreeContext.Provider value={getGitColor}>
      <ScrollArea className="h-full font-ui">
        <div className="pt-1 pb-4 min-h-full" data-file-tree>
          <FileTreeNode
            entry={rootEntry}
            depth={0}
            paneId={paneId}
            defaultExpanded
            preloadedChildren={entries}
            headerActions={rootActions}
          />
        </div>
      </ScrollArea>
      {externalDrop &&
        createPortal(
          <div
            className="fixed bg-[var(--color-accent-blue-muted)] border border-[var(--color-accent-blue)] pointer-events-none z-50"
            style={{
              left: externalDrop.rect.left,
              top: externalDrop.rect.top,
              width: externalDrop.rect.width,
              height: externalDrop.rect.height,
            }}
          />,
          document.body,
        )}
    </GitFileTreeContext.Provider>
  );
}
