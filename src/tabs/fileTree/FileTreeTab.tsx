import { useEffect, useCallback, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useActiveWorkspace, useIsWorkspaceActive } from "../../contexts/WorkspaceContext";
import { FileTreeNode } from "./FileTreeNode";
import { useFileTreeSelection } from "./fileTreeStores";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { StateView } from "../../components/shared/StateView";
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
