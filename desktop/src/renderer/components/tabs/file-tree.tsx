import type {
  ContextMenuItem as FileTreeContextMenuItem,
  ContextMenuOpenContext as FileTreeContextMenuOpenContext,
  FileTree as FileTreeModel,
  FileTreeDirectoryHandle,
  FileTreeDropResult,
  FileTreeItemHandle,
  FileTreeRenameEvent,
} from "@pierre/trees";
import { FileTree as PierreFileTree, useFileTree } from "@pierre/trees/react";
import type { MouseEvent, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import {
  copyFileTreeEntries,
  createFileTreeEntry,
  deleteFileTreeEntries,
  getFileTree,
  getFileTreeChildren,
  moveFileTreeEntries,
  renameFileTreeEntry,
  revealFileTreePath,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import { useWorkspaceStore } from "@/renderer/stores";
import type { FileTreeEntryKind, FileTreeSnapshot, TabId, WorkspaceId } from "@/shared/ipc";

type FileTreeTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  onActivatePane(): void;
};

type FileTreeLoadState =
  | { status: "loading"; workspaceId: WorkspaceId; tabId: TabId }
  | {
      status: "loaded";
      workspaceId: WorkspaceId;
      tabId: TabId;
      snapshot: FileTreeSnapshot;
      revision: number;
    }
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string };

type FileTreeClipboard = {
  action: "copy" | "cut";
  paths: string[];
};

type PendingFileTreeCreate = {
  kind: FileTreeEntryKind;
};

type RootContextMenuPosition = {
  x: number;
  y: number;
};

const EXPANSION_SAVE_DELAY_MS = 250;

export function FileTreeTab({ workspaceId, tabId, onActivatePane }: FileTreeTabProps) {
  const [loadState, setLoadState] = useState<FileTreeLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });
  const requestIdRef = useRef(0);
  const revisionRef = useRef(0);

  const loadFileTree = async (targetWorkspaceId: WorkspaceId, targetTabId: TabId, showLoading: boolean) => {
    const requestId = requestIdRef.current + 1;

    requestIdRef.current = requestId;

    if (showLoading) {
      setLoadState({ status: "loading", workspaceId: targetWorkspaceId, tabId: targetTabId });
    }

    try {
      const snapshot = await getFileTree({ workspaceId: targetWorkspaceId, tabId: targetTabId });

      if (requestIdRef.current === requestId) {
        revisionRef.current += 1;
        setLoadState({
          status: "loaded",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          snapshot,
          revision: revisionRef.current,
        });
      }
    } catch (caughtError: unknown) {
      if (requestIdRef.current === requestId) {
        setLoadState({
          status: "error",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          message: errorMessage(caughtError),
        });
      }
    }
  };

  useEffect(() => {
    void loadFileTree(workspaceId, tabId, true);
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
          key={currentLoadState.revision}
          workspaceId={workspaceId}
          tabId={tabId}
          snapshot={currentLoadState.snapshot}
          onReload={() => loadFileTree(workspaceId, tabId, false)}
        />
      ) : null}
    </div>
  );
}

function LoadedFileTree({
  workspaceId,
  tabId,
  snapshot,
  onReload,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  snapshot: FileTreeSnapshot;
  onReload(): Promise<void>;
}) {
  const [clipboard, setClipboard] = useState<FileTreeClipboard | null>(null);
  const [rootContextMenuPosition, setRootContextMenuPosition] =
    useState<RootContextMenuPosition | null>(null);
  const [hasInlineCreate, setHasInlineCreate] = useState(false);
  const pendingCreatesRef = useRef<Map<string, PendingFileTreeCreate>>(new Map());
  const closeRowContextMenuRef = useRef<FileTreeContextMenuOpenContext["close"] | null>(null);
  const runMutation = async (mutation: () => Promise<unknown>) => {
    try {
      await mutation();
    } catch (caughtError: unknown) {
      window.alert(errorMessage(caughtError));
    } finally {
      await onReload();
    }
  };
  const moveDroppedEntries = (event: FileTreeDropResult) => {
    void runMutation(() =>
      moveFileTreeEntries({
        workspaceId,
        tabId,
        sourcePaths: [...event.draggedPaths],
        targetDirectoryPath: targetDirectoryPath(event.target.directoryPath),
      }),
    );
  };
  const renameEntry = (event: FileTreeRenameEvent) => {
    const pendingCreate = pendingCreatesRef.current.get(event.sourcePath);

    if (pendingCreate) {
      pendingCreatesRef.current.delete(event.sourcePath);
      void runMutation(() =>
        createFileTreeEntry({
          workspaceId,
          tabId,
          parentPath: parentDirectoryPath(event.destinationPath),
          name: treePathBasename(event.destinationPath),
          kind: pendingCreate.kind,
        }),
      ).finally(() => setHasInlineCreate(false));
      return;
    }

    void runMutation(() =>
      renameFileTreeEntry({
        workspaceId,
        tabId,
        sourcePath: event.sourcePath,
        destinationPath: event.destinationPath,
      }),
    );
  };
  const { model } = useFileTree({
    dragAndDrop: {
      canDrag: (paths) => paths.length > 0,
      canDrop: (event) => event.target.kind === "root" || event.target.directoryPath !== null,
      onDropComplete: moveDroppedEntries,
      onDropError: (message) => window.alert(message),
    },
    density: "compact",
    flattenEmptyDirectories: true,
    initialExpandedPaths: snapshot.expandedPaths,
    initialExpansion: "closed",
    paths: snapshot.paths,
    renaming: {
      canRename: () => true,
      onError: (message) => {
        removePendingCreates();
        window.alert(message);
      },
      onRename: renameEntry,
    },
  });
  useEffect(() => {
    return model.onMutation("*", (event) => {
      if (event.operation === "remove") {
        clearPendingCreate(event.path);
      }

      if (event.operation === "batch") {
        for (const mutationEvent of event.events) {
          if (mutationEvent.operation === "remove") {
            clearPendingCreate(mutationEvent.path);
          }
        }
      }
    });
  }, [model]);
  useEffect(() => {
    const shadowRoot = model.getFileTreeContainer()?.shadowRoot;

    if (!shadowRoot) {
      return undefined;
    }

    const style = document.createElement("style");
    style.dataset.kosmosFileTreeCutFeedback = "true";
    style.textContent = cutFeedbackCss(clipboard);

    if (style.textContent.length === 0) {
      return undefined;
    }

    shadowRoot.appendChild(style);

    return () => {
      style.remove();
    };
  }, [clipboard, model]);
  const closeRootContextMenu = () => {
    setRootContextMenuPosition(null);
  };
  const registerRowContextMenu = (context: FileTreeContextMenuOpenContext) => {
    closeRootContextMenu();
    closeRowContextMenuRef.current = context.close;

    return () => {
      if (closeRowContextMenuRef.current === context.close) {
        closeRowContextMenuRef.current = null;
      }
    };
  };
  const clearPendingCreate = (path: string) => {
    if (pendingCreatesRef.current.delete(renameSourcePath(path))) {
      setHasInlineCreate(pendingCreatesRef.current.size > 0);
    }
  };
  const isPendingCreatePath = (path: string): boolean => {
    return pendingCreatesRef.current.has(renameSourcePath(path));
  };
  const removePendingCreate = (path: string): boolean => {
    const sourcePath = renameSourcePath(path);
    const pendingCreate = pendingCreatesRef.current.get(sourcePath);

    if (!pendingCreate) {
      return false;
    }

    pendingCreatesRef.current.delete(sourcePath);
    setHasInlineCreate(pendingCreatesRef.current.size > 0);

    const placeholderPath =
      pendingCreate.kind === "directory" ? `${sourcePath}/` : sourcePath;
    if (model.getItem(placeholderPath)) {
      model.remove(
        placeholderPath,
        pendingCreate.kind === "directory" ? { recursive: true } : undefined,
      );
    }

    return true;
  };
  const removePendingCreates = () => {
    for (const [sourcePath, pendingCreate] of [...pendingCreatesRef.current]) {
      removePendingCreate(pendingCreate.kind === "directory" ? `${sourcePath}/` : sourcePath);
    }
  };
  const startInlineCreate = (kind: FileTreeEntryKind, parentPath: string | null) => {
    const placeholderPath = nextUntitledPath(model, parentPath, kind);
    const sourcePath = renameSourcePath(placeholderPath);

    pendingCreatesRef.current.set(sourcePath, { kind });
    setHasInlineCreate(true);

    const parent = parentPath ? model.getItem(parentPath) : null;
    if (isDirectoryHandle(parent)) {
      parent.expand();
    }

    try {
      model.add(placeholderPath);

      if (!model.startRenaming(placeholderPath, { removeIfCanceled: true })) {
        pendingCreatesRef.current.delete(sourcePath);
        model.remove(placeholderPath, kind === "directory" ? { recursive: true } : undefined);
        setHasInlineCreate(false);
      }
    } catch (caughtError: unknown) {
      pendingCreatesRef.current.delete(sourcePath);
      setHasInlineCreate(false);
      window.alert(errorMessage(caughtError));
    }
  };
  const openRootContextMenu = (event: MouseEvent<HTMLDivElement>) => {
    if (event.defaultPrevented || contextMenuStartedOnTreeItem(event)) {
      closeRootContextMenu();
      return;
    }

    event.preventDefault();
    closeRowContextMenuRef.current?.({ restoreFocus: false });
    setRootContextMenuPosition({ x: event.clientX, y: event.clientY });
  };

  useEffect(() => {
    if (!rootContextMenuPosition) {
      return undefined;
    }

    const closeOnPointerDown = () => {
      closeRootContextMenu();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeRootContextMenu();
      }
    };

    window.addEventListener("pointerdown", closeOnPointerDown);
    window.addEventListener("keydown", closeOnEscape);

    return () => {
      window.removeEventListener("pointerdown", closeOnPointerDown);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [rootContextMenuPosition]);

  return (
    <div className="relative h-full min-h-0 overflow-hidden" onContextMenu={openRootContextMenu}>
      <PierreFileTree
        model={model}
        className="h-full min-h-0 w-full overflow-hidden bg-card text-card-foreground [--trees-accent-override:var(--accent)] [--trees-bg-muted-override:var(--accent)] [--trees-bg-override:var(--card)] [--trees-border-color-override:var(--border)] [--trees-fg-muted-override:var(--muted-foreground)] [--trees-fg-override:var(--card-foreground)] [--trees-focus-ring-color-override:var(--ring)] [--trees-input-bg-override:var(--input)] [--trees-item-row-gap-override:6px] [--trees-padding-inline-override:0px] [--trees-scrollbar-gutter-override:0px] [--trees-search-bg-override:var(--input)] [--trees-search-fg-override:var(--foreground)] [--trees-selected-bg-override:var(--accent)] [--trees-selected-fg-override:var(--accent-foreground)] [--trees-selected-focused-border-color-override:var(--ring)]"
        style={{ height: "100%" }}
        renderContextMenu={(item, context) => (
          createPortal(
            <FileTreeContextMenu
              clipboard={clipboard}
              context={context}
              item={item}
              model={model}
              tabId={tabId}
              workspaceId={workspaceId}
              onClipboardChange={setClipboard}
              onCreateInline={startInlineCreate}
              onIsPendingCreate={isPendingCreatePath}
              onRegisterContextMenu={registerRowContextMenu}
              onRemovePendingCreate={removePendingCreate}
              onMutation={runMutation}
            />,
            document.body,
          )
        )}
      />
      {snapshot.paths.length === 0 && !hasInlineCreate ? (
        <div className="pointer-events-none absolute inset-0">
          <FileTreeMessage message="This workspace is empty." />
        </div>
      ) : null}
      <FileTreeExpansionPersistence
        model={model}
        snapshot={snapshot}
        workspaceId={workspaceId}
        tabId={tabId}
      />
      <FileTreeDeferredDirectoryLoader
        model={model}
        snapshot={snapshot}
        workspaceId={workspaceId}
        tabId={tabId}
      />
      {rootContextMenuPosition ? (
        <RootFileTreeContextMenu
          clipboard={clipboard}
          position={rootContextMenuPosition}
          tabId={tabId}
          workspaceId={workspaceId}
          onClipboardChange={setClipboard}
          onClose={closeRootContextMenu}
          onCreateInline={startInlineCreate}
          onMutation={runMutation}
        />
      ) : null}
    </div>
  );
}

function RootFileTreeContextMenu({
  clipboard,
  position,
  tabId,
  workspaceId,
  onClipboardChange,
  onClose,
  onCreateInline,
  onMutation,
}: {
  clipboard: FileTreeClipboard | null;
  position: RootContextMenuPosition;
  tabId: TabId;
  workspaceId: WorkspaceId;
  onClipboardChange(clipboard: FileTreeClipboard | null): void;
  onClose(): void;
  onCreateInline(kind: FileTreeEntryKind, parentPath: string | null): void;
  onMutation(mutation: () => Promise<unknown>): Promise<void>;
}) {
  const closeAndRun = (mutation: () => Promise<unknown>) => {
    onClose();
    void onMutation(mutation);
  };

  return (
    <div
      data-file-tree-context-menu-root="true"
      role="menu"
      tabIndex={-1}
      className="fixed z-[1000] min-w-40 rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 outline-none"
      style={{ left: position.x, top: position.y }}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          onClose();
        }
      }}
      onPointerDown={(event) => {
        event.stopPropagation();
      }}
    >
      <FileTreeMenuItem
        onSelect={() => {
          onClose();
          onCreateInline("file", null);
        }}
      >
        New file
      </FileTreeMenuItem>
      <FileTreeMenuItem
        onSelect={() => {
          onClose();
          onCreateInline("directory", null);
        }}
      >
        New folder
      </FileTreeMenuItem>
      <FileTreeMenuSeparator />
      <FileTreeMenuItem
        disabled={!clipboard}
        onSelect={() => {
          if (!clipboard) {
            return;
          }

          closeAndRun(async () => {
            if (clipboard.action === "copy") {
              await copyFileTreeEntries({
                workspaceId,
                tabId,
                sourcePaths: clipboard.paths,
                targetDirectoryPath: null,
              });
            } else {
              await moveFileTreeEntries({
                workspaceId,
                tabId,
                sourcePaths: clipboard.paths,
                targetDirectoryPath: null,
              });
              onClipboardChange(null);
            }
          });
        }}
      >
        Paste
      </FileTreeMenuItem>
      <FileTreeMenuSeparator />
      <FileTreeMenuItem
        onSelect={() => {
          onClose();
          void revealFileTreePath({ workspaceId, tabId }).catch((caughtError: unknown) =>
            window.alert(errorMessage(caughtError)),
          );
        }}
      >
        Reveal in file manager
      </FileTreeMenuItem>
    </div>
  );
}
function FileTreeContextMenu({
  clipboard,
  context,
  item,
  model,
  tabId,
  workspaceId,
  onClipboardChange,
  onCreateInline,
  onIsPendingCreate,
  onRegisterContextMenu,
  onRemovePendingCreate,
  onMutation,
}: {
  clipboard: FileTreeClipboard | null;
  context: FileTreeContextMenuOpenContext;
  item: FileTreeContextMenuItem;
  model: FileTreeModel;
  tabId: TabId;
  workspaceId: WorkspaceId;
  onClipboardChange(clipboard: FileTreeClipboard | null): void;
  onCreateInline(kind: FileTreeEntryKind, parentPath: string | null): void;
  onIsPendingCreate(path: string): boolean;
  onRegisterContextMenu(context: FileTreeContextMenuOpenContext): () => void;
  onRemovePendingCreate(path: string): boolean;
  onMutation(mutation: () => Promise<unknown>): Promise<void>;
}) {
  const itemTargetDirectoryPath = targetDirectoryPathForItem(item);
  const itemIsPendingCreate = onIsPendingCreate(item.path);
  const selectedPaths = selectedOrItemPaths(model, item.path);
  const selectedCount = selectedPaths.length;
  const selectedPendingPaths = selectedPaths.filter(onIsPendingCreate);
  const selectedRealPaths = selectedPaths.filter((path) => !onIsPendingCreate(path));
  const hasPendingSelection = selectedPendingPaths.length > 0;
  const closeAndRun = (mutation: () => Promise<unknown>) => {
    context.close();
    void onMutation(mutation);
  };

  useEffect(() => onRegisterContextMenu(context), [context, onRegisterContextMenu]);

  return (
    <div
      data-file-tree-context-menu-root="true"
      role="menu"
      tabIndex={-1}
      className="fixed z-[1000] min-w-40 rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 outline-none"
      style={{ left: context.anchorRect.left, top: context.anchorRect.top }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          context.close();
        }
      }}
    >
      <FileTreeMenuItem
        onSelect={() => {
          context.close({ restoreFocus: false });
          onCreateInline("file", itemTargetDirectoryPath);
        }}
      >
        New file
      </FileTreeMenuItem>
      <FileTreeMenuItem
        onSelect={() => {
          context.close({ restoreFocus: false });
          onCreateInline("directory", itemTargetDirectoryPath);
        }}
      >
        New folder
      </FileTreeMenuItem>
      <FileTreeMenuSeparator />
      <FileTreeMenuItem
        disabled={selectedCount !== 1}
        onSelect={() => {
          context.close({ restoreFocus: false });
          model.startRenaming(item.path);
        }}
      >
        Rename
      </FileTreeMenuItem>
      <FileTreeMenuItem
        disabled={hasPendingSelection}
        onSelect={() => {
          onClipboardChange({ action: "copy", paths: selectedPaths });
          context.close();
        }}
      >
        {selectedCount === 1 ? "Copy" : `Copy ${selectedCount} items`}
      </FileTreeMenuItem>
      <FileTreeMenuItem
        disabled={hasPendingSelection}
        onSelect={() => {
          onClipboardChange({ action: "cut", paths: selectedPaths });
          context.close();
        }}
      >
        {selectedCount === 1 ? "Cut" : `Cut ${selectedCount} items`}
      </FileTreeMenuItem>
      <FileTreeMenuItem
        disabled={!clipboard}
        onSelect={() => {
          if (!clipboard) {
            return;
          }

          closeAndRun(async () => {
            if (clipboard.action === "copy") {
              await copyFileTreeEntries({
                workspaceId,
                tabId,
                sourcePaths: clipboard.paths,
                targetDirectoryPath: itemTargetDirectoryPath,
              });
            } else {
              await moveFileTreeEntries({
                workspaceId,
                tabId,
                sourcePaths: clipboard.paths,
                targetDirectoryPath: itemTargetDirectoryPath,
              });
              onClipboardChange(null);
            }
          });
        }}
      >
        Paste
      </FileTreeMenuItem>
      <FileTreeMenuSeparator />
      <FileTreeMenuItem
        disabled={itemIsPendingCreate}
        onSelect={() => {
          context.close();
          void revealFileTreePath({ workspaceId, tabId, path: item.path }).catch(
            (caughtError: unknown) => window.alert(errorMessage(caughtError)),
          );
        }}
      >
        Reveal in file manager
      </FileTreeMenuItem>
      <FileTreeMenuSeparator />
      <FileTreeMenuItem
        variant="destructive"
        onSelect={() => {
          if (selectedRealPaths.length === 0) {
            context.close();
            selectedPendingPaths.forEach(onRemovePendingCreate);
            return;
          }

          const deleteCount = selectedRealPaths.length + selectedPendingPaths.length;
          const message =
            deleteCount === 1 ? `Delete ${item.name}?` : `Delete ${deleteCount} items?`;

          if (!window.confirm(message)) {
            context.close();
            return;
          }

          closeAndRun(async () => {
            selectedPendingPaths.forEach(onRemovePendingCreate);
            await deleteFileTreeEntries({ workspaceId, tabId, paths: selectedRealPaths });
          });
        }}
      >
        {selectedCount === 1 ? "Delete" : `Delete ${selectedCount} items`}
      </FileTreeMenuItem>
    </div>
  );
}

function FileTreeMenuItem({
  children,
  disabled = false,
  variant = "default",
  onSelect,
}: {
  children: ReactNode;
  disabled?: boolean;
  variant?: "default" | "destructive";
  onSelect(): void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      data-variant={variant}
      className="relative flex w-full cursor-default items-center rounded-md px-1.5 py-1 text-left text-sm outline-hidden select-none focus:bg-accent focus:text-accent-foreground data-[variant=destructive]:text-destructive data-[variant=destructive]:focus:bg-destructive/10 data-[variant=destructive]:focus:text-destructive disabled:pointer-events-none disabled:opacity-50"
      onPointerDown={(event) => {
        event.stopPropagation();
      }}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        onSelect();
      }}
    >
      {children}
    </button>
  );
}

function FileTreeMenuSeparator() {
  return <div role="separator" className="-mx-1 my-1 h-px bg-border" />;
}

function selectedOrItemPaths(model: FileTreeModel, itemPath: string): string[] {
  const selectedPaths = model.getSelectedPaths();
  const paths = selectedPaths.includes(itemPath) ? [...selectedPaths] : [itemPath];

  return normalizeSelectedTreePaths(paths);
}

function normalizeSelectedTreePaths(paths: string[]): string[] {
  const sortedPaths = [...new Set(paths)].sort((left, right) => {
    const depthDifference = treePathDepth(left) - treePathDepth(right);

    return depthDifference === 0 ? left.localeCompare(right) : depthDifference;
  });
  const normalizedPaths: string[] = [];

  for (const path of sortedPaths) {
    if (normalizedPaths.some((selectedPath) => treePathContains(selectedPath, path))) {
      continue;
    }

    normalizedPaths.push(path);
  }

  return normalizedPaths;
}

function treePathContains(parentPath: string, childPath: string): boolean {
  return parentPath.endsWith("/") && childPath !== parentPath && childPath.startsWith(parentPath);
}

function treePathDepth(path: string): number {
  return renameSourcePath(path).split("/").length;
}

function targetDirectoryPathForItem(item: FileTreeContextMenuItem): string | null {
  if (item.kind === "directory") {
    return targetDirectoryPath(item.path);
  }

  return parentDirectoryPath(item.path);
}

function targetDirectoryPath(path: string | null): string | null {
  return path && path.length > 0 ? path : null;
}

function cutFeedbackCss(clipboard: FileTreeClipboard | null): string {
  if (clipboard?.action !== "cut") {
    return "";
  }

  return clipboard.paths
    .map((path) => {
      const selector = `[data-type="item"][data-item-path=${cssString(path)}]`;

      return `${selector}{opacity:.45}${selector} [data-item-section="content"]{text-decoration:line-through}`;
    })
    .join("\n");
}

function cssString(value: string): string {
  return JSON.stringify(value);
}

function contextMenuStartedOnTreeItem(event: MouseEvent<HTMLDivElement>): boolean {
  for (const entry of event.nativeEvent.composedPath()) {
    if (!(entry instanceof HTMLElement)) {
      continue;
    }

    if (
      entry.dataset.type === "item" ||
      entry.dataset.type === "context-menu-anchor" ||
      entry.dataset.type === "context-menu-trigger" ||
      entry.dataset.itemPath !== undefined
    ) {
      return true;
    }
  }

  return false;
}

function nextUntitledPath(
  model: FileTreeModel,
  parentPath: string | null,
  kind: FileTreeEntryKind,
): string {
  for (let index = 1; ; index += 1) {
    const name = index === 1 ? "untitled" : `untitled ${index}`;
    const path = placeholderPath(parentPath, name, kind);

    if (!model.getItem(path)) {
      return path;
    }
  }
}

function placeholderPath(
  parentPath: string | null,
  name: string,
  kind: FileTreeEntryKind,
): string {
  const path = parentPath ? `${parentPath}${name}` : name;

  return kind === "directory" ? `${path}/` : path;
}

function renameSourcePath(path: string): string {
  return path.endsWith("/") ? path.slice(0, -1) : path;
}

function treePathBasename(path: string): string {
  const normalizedPath = renameSourcePath(path);
  const separatorIndex = normalizedPath.lastIndexOf("/");

  return separatorIndex < 0 ? normalizedPath : normalizedPath.slice(separatorIndex + 1);
}

function parentDirectoryPath(path: string): string | null {
  const normalizedPath = path.endsWith("/") ? path.slice(0, -1) : path;
  const separatorIndex = normalizedPath.lastIndexOf("/");

  if (separatorIndex < 0) {
    return null;
  }

  return `${normalizedPath.slice(0, separatorIndex)}/`;
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

function FileTreeDeferredDirectoryLoader({
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
  const deferredPathsRef = useRef(new Set(snapshot.deferredPaths));
  const loadingPathsRef = useRef(new Set<string>());

  useEffect(() => {
    deferredPathsRef.current = new Set(snapshot.deferredPaths);
    loadingPathsRef.current.clear();
  }, [snapshot.deferredPaths]);

  useEffect(() => {
    let disposed = false;

    const loadDirectory = async (path: string) => {
      loadingPathsRef.current.add(path);

      try {
        const children = await getFileTreeChildren({ workspaceId, tabId, path });

        if (disposed) {
          return;
        }

        const operations = children.paths
          .filter((childPath) => !model.getItem(childPath))
          .map((childPath) => ({ type: "add" as const, path: childPath }));

        deferredPathsRef.current.delete(path);

        for (const deferredPath of children.deferredPaths) {
          deferredPathsRef.current.add(deferredPath);
        }

        if (operations.length > 0) {
          model.batch(operations);
        }
      } catch (caughtError: unknown) {
        if (!disposed) {
          window.alert(errorMessage(caughtError));
        }
      } finally {
        loadingPathsRef.current.delete(path);
      }
    };

    const loadExpandedDeferredDirectories = () => {
      for (const path of deferredPathsRef.current) {
        if (loadingPathsRef.current.has(path)) {
          continue;
        }

        const item = model.getItem(path);
        if (isDirectoryHandle(item) && item.isExpanded()) {
          void loadDirectory(path);
        }
      }
    };

    loadExpandedDeferredDirectories();

    const unsubscribe = model.subscribe(loadExpandedDeferredDirectories);

    return () => {
      disposed = true;
      unsubscribe();
    };
  }, [model, workspaceId, tabId]);

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
