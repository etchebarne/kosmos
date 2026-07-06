import type {
  FileTree as FileTreeModel,
  FileTreeDirectoryHandle,
  FileTreeItemHandle,
} from "@pierre/trees";
import { FileTree as PierreFileTree, useFileTree } from "@pierre/trees/react";
import { useEffect, useRef, useState } from "react";

import { getFileTree } from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import { useWorkspaceStore } from "@/renderer/stores";
import type { FileTreeSnapshot, TabId, WorkspaceId } from "@/shared/ipc";

type FileTreeTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  onActivatePane(): void;
};

type FileTreeLoadState =
  | { status: "loading"; workspaceId: WorkspaceId; tabId: TabId }
  | { status: "loaded"; workspaceId: WorkspaceId; tabId: TabId; snapshot: FileTreeSnapshot }
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string };

const EXPANSION_SAVE_DELAY_MS = 250;

export function FileTreeTab({ workspaceId, tabId, onActivatePane }: FileTreeTabProps) {
  const [loadState, setLoadState] = useState<FileTreeLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });

  useEffect(() => {
    let isCurrent = true;

    setLoadState({ status: "loading", workspaceId, tabId });

    void getFileTree({ workspaceId, tabId })
      .then((snapshot) => {
        if (isCurrent) {
          setLoadState({ status: "loaded", workspaceId, tabId, snapshot });
        }
      })
      .catch((caughtError: unknown) => {
        if (isCurrent) {
          setLoadState({ status: "error", workspaceId, tabId, message: errorMessage(caughtError) });
        }
      });

    return () => {
      isCurrent = false;
    };
  }, [workspaceId, tabId]);

  const currentLoadState: FileTreeLoadState =
    loadState.workspaceId === workspaceId && loadState.tabId === tabId
      ? loadState
      : { status: "loading", workspaceId, tabId };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden" onPointerDown={onActivatePane}>
      {currentLoadState.status === "loading" ? <FileTreeMessage message="Loading files..." /> : null}
      {currentLoadState.status === "error" ? (
        <FileTreeMessage message={currentLoadState.message} />
      ) : null}
      {currentLoadState.status === "loaded" ? (
        <LoadedFileTree
          workspaceId={workspaceId}
          tabId={tabId}
          snapshot={currentLoadState.snapshot}
        />
      ) : null}
    </div>
  );
}

function LoadedFileTree({
  workspaceId,
  tabId,
  snapshot,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  snapshot: FileTreeSnapshot;
}) {
  const { model } = useFileTree({
    density: "compact",
    flattenEmptyDirectories: true,
    initialExpandedPaths: snapshot.expandedPaths,
    initialExpansion: "closed",
    paths: snapshot.paths,
  });

  if (snapshot.paths.length === 0) {
    return <FileTreeMessage message="This workspace has no files to show." />;
  }

  return (
    <>
      <PierreFileTree
        model={model}
        className="h-full min-h-0 w-full overflow-hidden bg-card text-card-foreground [--trees-bg-muted-override:var(--accent)] [--trees-bg-override:var(--card)] [--trees-border-color-override:var(--border)] [--trees-fg-muted-override:var(--muted-foreground)] [--trees-fg-override:var(--card-foreground)] [--trees-focus-ring-color-override:var(--ring)] [--trees-input-bg-override:var(--input)] [--trees-item-row-gap-override:6px] [--trees-padding-inline-override:0px] [--trees-scrollbar-gutter-override:0px] [--trees-search-bg-override:var(--input)] [--trees-search-fg-override:var(--foreground)] [--trees-selected-bg-override:var(--accent)] [--trees-selected-fg-override:var(--accent-foreground)]"
        style={{ height: "100%" }}
      />
      <FileTreeExpansionPersistence
        model={model}
        snapshot={snapshot}
        workspaceId={workspaceId}
        tabId={tabId}
      />
    </>
  );
}

function FileTreeExpansionPersistence({
  model,
  snapshot,
  workspaceId,
  tabId,
}: {
  model: FileTreeModel;
  snapshot: FileTreeSnapshot;
  workspaceId: WorkspaceId;
  tabId: TabId;
}) {
  const registerFileTreeExpansionFlusher = useWorkspaceStore(
    (state) => state.registerFileTreeExpansionFlusher,
  );
  const saveFileTreeExpandedPaths = useWorkspaceStore((state) => state.saveFileTreeExpandedPaths);
  const persistedExpandedPathsRef = useRef(sortedUnique(snapshot.expandedPaths));
  const pendingExpandedPathsRef = useRef<string[] | null>(null);

  useEffect(() => {
    persistedExpandedPathsRef.current = sortedUnique(snapshot.expandedPaths);
    pendingExpandedPathsRef.current = null;
  }, [snapshot.expandedPaths]);

  useEffect(() => {
    const directoryPaths = snapshot.paths.filter((path) => path.endsWith("/"));
    let saveTimeout: ReturnType<typeof setTimeout> | null = null;

    const persistExpandedPaths = (expandedPaths: string[]) => {
      return saveFileTreeExpandedPaths({ workspaceId, tabId, expandedPaths });
    };

    const saveExpandedPaths = (): Promise<void> | void => {
      const expandedPaths = expandedDirectoryPaths(model, directoryPaths);
      const targetExpandedPaths = pendingExpandedPathsRef.current ?? persistedExpandedPathsRef.current;
      if (stringArraysEqual(targetExpandedPaths, expandedPaths)) {
        return;
      }

      pendingExpandedPathsRef.current = expandedPaths;

      if (saveTimeout !== null) {
        clearTimeout(saveTimeout);
      }

      saveTimeout = setTimeout(() => {
        const pendingExpandedPaths = pendingExpandedPathsRef.current;

        saveTimeout = null;
        pendingExpandedPathsRef.current = null;

        if (
          pendingExpandedPaths &&
          !stringArraysEqual(persistedExpandedPathsRef.current, pendingExpandedPaths)
        ) {
          persistedExpandedPathsRef.current = pendingExpandedPaths;
          void persistExpandedPaths(pendingExpandedPaths);
        }
      }, EXPANSION_SAVE_DELAY_MS);
    };

    const flushExpandedPaths = () => {
      const expandedPaths = expandedDirectoryPaths(model, directoryPaths);

      if (saveTimeout !== null) {
        clearTimeout(saveTimeout);
        saveTimeout = null;
      }

      pendingExpandedPathsRef.current = null;

      if (stringArraysEqual(persistedExpandedPathsRef.current, expandedPaths)) {
        return undefined;
      }

      persistedExpandedPathsRef.current = expandedPaths;

      return persistExpandedPaths(expandedPaths);
    };

    const unsubscribe = model.subscribe(saveExpandedPaths);
    const unregisterFlusher = registerFileTreeExpansionFlusher(flushExpandedPaths);

    return () => {
      unsubscribe();
      unregisterFlusher();
      void flushExpandedPaths();
    };
  }, [
    model,
    registerFileTreeExpansionFlusher,
    saveFileTreeExpandedPaths,
    snapshot.paths,
    workspaceId,
    tabId,
  ]);

  return null;
}

function FileTreeMessage({ message }: { message: string }) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <p className="max-w-sm text-sm text-muted-foreground">{message}</p>
    </div>
  );
}

function expandedDirectoryPaths(model: FileTreeModel, directoryPaths: string[]): string[] {
  return sortedUnique(
    directoryPaths.filter((path) => {
      const item = model.getItem(path);
      if (!isDirectoryHandle(item)) {
        return false;
      }

      return item.isExpanded() && ancestorDirectoriesAreExpanded(model, path);
    }),
  );
}

function ancestorDirectoriesAreExpanded(model: FileTreeModel, path: string): boolean {
  for (const ancestorPath of ancestorDirectoryPaths(path)) {
    const ancestor = model.getItem(ancestorPath);

    if (!isDirectoryHandle(ancestor) || !ancestor.isExpanded()) {
      return false;
    }
  }

  return true;
}

function ancestorDirectoryPaths(path: string): string[] {
  const segments = path.replace(/\/$/, "").split("/");
  const ancestors: string[] = [];

  for (let index = 1; index < segments.length; index += 1) {
    ancestors.push(`${segments.slice(0, index).join("/")}/`);
  }

  return ancestors;
}

function sortedUnique(paths: string[]): string[] {
  return [...new Set(paths)].sort();
}

function stringArraysEqual(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }

  return left.every((value, index) => value === right[index]);
}

function isDirectoryHandle(
  item: FileTreeItemHandle | null,
): item is FileTreeDirectoryHandle {
  return item?.isDirectory() === true;
}
