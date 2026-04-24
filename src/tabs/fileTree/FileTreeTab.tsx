import { useEffect, useCallback, useRef, useState, useMemo } from "react";
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
import { useIsTabActive } from "../../hooks/useIsTabActive";
import { GitFileTreeContext, buildGitColorLookup } from "./gitFileTreeContext";
import { getCached, getOrFetch, invalidate } from "./fileTreeCache";
import type { TabContentProps } from "../types";

export interface DirEntry {
  name: string;
  path: string;
  isDir: boolean;
  extension: string | null;
}

export function FileTreeTab({ tab, paneId }: TabContentProps) {
  const activeWorkspace = useActiveWorkspace();
  const isActive = useIsWorkspaceActive();
  const isTabActive = useIsTabActive(paneId, tab.id);
  const isVisible = isActive && isTabActive;
  const workspacePath = activeWorkspace?.path ?? null;
  const treeRef = useRef<HTMLDivElement | null>(null);

  const initialEntries = workspacePath ? getCached(workspacePath) : null;
  const [entries, setEntries] = useState<DirEntry[]>(initialEntries ?? []);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!initialEntries);
  const [externalDrop, setExternalDrop] = useState<{ dirPath: string; rect: DOMRect } | null>(null);
  const pendingRefreshDirsRef = useRef(new Set<string>());
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { status: gitStatus } = useGitStatus(workspacePath, isVisible);

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
    if (!isVisible) return;
    loadRoot();
  }, [isVisible, loadRoot]);

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

  useEffect(() => {
    if (!workspacePath || !isVisible) return;

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
  }, [workspacePath, isVisible]);

  const getExpandedDirs = useCallback(() => {
    const dirs = new Set<string>();
    if (workspacePath) dirs.add(workspacePath);
    treeRef.current
      ?.querySelectorAll<HTMLElement>("[data-tree-dir][data-tree-expanded='true']")
      .forEach((el) => {
        const dirPath = el.dataset.treeDir;
        if (dirPath) dirs.add(dirPath);
      });
    return dirs;
  }, [workspacePath]);

  const flushPendingRefreshes = useCallback(() => {
    if (!isVisible || pendingRefreshDirsRef.current.size === 0) return;
    const pending = pendingRefreshDirsRef.current;
    const expandedDirs = getExpandedDirs();
    pendingRefreshDirsRef.current = new Set();
    for (const dir of pending) {
      if (!expandedDirs.has(dir)) continue;
      window.dispatchEvent(new CustomEvent("file-tree-refresh", { detail: { dir } }));
    }
  }, [getExpandedDirs, isVisible]);

  // One listener fans events out via CustomEvents; per-node listeners caused
  // a read_dir thundering herd that starved the runtime.
  useEffect(() => {
    const unlisten = listen<string[]>("file-tree-changed", (event) => {
      for (const dir of event.payload) {
        invalidate(dir);
        pendingRefreshDirsRef.current.add(dir);
      }
      if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = setTimeout(() => {
        refreshTimerRef.current = null;
        flushPendingRefreshes();
      }, 300);
    });

    return () => {
      if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current);
      unlisten.then((fn) => fn());
    };
  }, [flushPendingRefreshes]);

  useEffect(() => {
    flushPendingRefreshes();
  }, [flushPendingRefreshes]);

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
    const dirs = getExpandedDirs();
    for (const dir of dirs) {
      invalidate(dir);
      window.dispatchEvent(new CustomEvent("file-tree-refresh", { detail: { dir } }));
    }
  }, [getExpandedDirs, workspacePath]);

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
        <div ref={treeRef} className="pt-1 pb-4 min-h-full" data-file-tree>
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
