import { useEffect, useCallback, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useActiveWorkspace, useIsWorkspaceActive } from "../../contexts/WorkspaceContext";
import { FileTreeNode } from "./FileTreeNode";
import { useFileTreeSelection } from "./file-tree-stores";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { StateView } from "../../components/shared/StateView";
import { useGitStatus } from "../../hooks/use-git-status";
import { GitFileTreeContext, buildGitColorLookup } from "./git-file-tree-context";
import { getCached, getOrFetch, invalidate } from "./file-tree-cache";
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

  // Serve from cache for instant render when prefetch has completed
  const initialEntries = workspacePath ? getCached(workspacePath) : null;
  const [entries, setEntries] = useState<DirEntry[]>(initialEntries ?? []);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!initialEntries);
  const { status: gitStatus } = useGitStatus(workspacePath, isActive);

  const getGitColor = useMemo(() => {
    if (!activeWorkspace || !gitStatus?.isRepo) return () => null;
    return buildGitColorLookup(gitStatus.changes, activeWorkspace.path);
  }, [activeWorkspace, gitStatus]);

  const loadRoot = useCallback(async () => {
    if (!workspacePath) return;
    // If cache already provided entries, skip redundant load
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
  }, [workspacePath]); // Depend on path string, not object reference

  useEffect(() => {
    loadRoot();
  }, [loadRoot]);

  // Clear selection when clicking outside the file tree
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

  // Start filesystem watcher only for the active workspace — the watcher is a
  // singleton, so inactive workspaces would replace the active one's watcher.
  useEffect(() => {
    if (!activeWorkspace || !isActive) return;
    invoke("watch_workspace", { path: activeWorkspace.path }).catch((e) =>
      console.warn("Failed to start file watcher:", e),
    );
    return () => {
      invoke("unwatch_workspace", { path: activeWorkspace.path });
    };
  }, [activeWorkspace, isActive]);

  // Single Tauri listener for watcher events, debounced and dispatched to nodes
  // via window CustomEvents. Replaces per-node Tauri listeners to avoid a
  // thundering herd of concurrent read_dir calls that can starve the runtime.
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
          />
        </div>
      </ScrollArea>
    </GitFileTreeContext.Provider>
  );
}
