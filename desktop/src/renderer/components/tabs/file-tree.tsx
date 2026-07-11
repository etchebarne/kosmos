import type {
  ContextMenuAnchorRect as FileTreeContextMenuAnchorRect,
  ContextMenuItem as FileTreeContextMenuItem,
  ContextMenuOpenContext as FileTreeContextMenuOpenContext,
  FileTree as FileTreeModel,
  FileTreeDirectoryHandle,
  FileTreeDropResult,
  FileTreeItemHandle,
  FileTreeRenameEvent,
  GitStatusEntry,
} from "@pierre/trees";
import { FileTree as PierreFileTree, useFileTree } from "@pierre/trees/react";
import { FilePlus2, FolderPlus, RefreshCw } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

import { Button } from "@/renderer/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
} from "@/renderer/components/ui/context-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/renderer/components/ui/tooltip";
import {
  copyFileTreeEntries,
  createFileTreeEntry,
  deleteFileTreeEntries,
  getFileTree,
  getFileTreeChildren,
  getFileTreeGitStatus,
  moveFileTreeEntries,
  renameFileTreeEntry,
  revealFileTreePath,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import { pierreGitStatus } from "@/renderer/lib/git-status";
import { useGitStore, useWorkspaceStore } from "@/renderer/stores";
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
      gitStatus: GitStatusEntry[];
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

type FileTreeContextMenuCloseOptions = {
  restoreFocus?: boolean;
};

type FileTreeContextMenuClose = (options?: FileTreeContextMenuCloseOptions) => void;

const EXPANSION_SAVE_DELAY_MS = 250;

export function FileTreeTab({ workspaceId, tabId, onActivatePane }: FileTreeTabProps) {
  const workspaceRevision = useGitStore((state) => state.revisions[workspaceId] ?? 0);
  const [loadState, setLoadState] = useState<FileTreeLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });
  const requestIdRef = useRef(0);
  const revisionRef = useRef(0);
  const snapshotSignatureRef = useRef<string | null>(null);
  const observedWorkspaceRevisionRef = useRef(workspaceRevision);
  const revisionLoadInFlightRef = useRef(false);
  const revisionLoadPendingRef = useRef(false);
  const revisionLoadTargetRef = useRef({ workspaceId, tabId });

  revisionLoadTargetRef.current = { workspaceId, tabId };

  const loadFileTree = async (
    targetWorkspaceId: WorkspaceId,
    targetTabId: TabId,
    showLoading: boolean,
    forceUpdate: boolean,
  ) => {
    const requestId = requestIdRef.current + 1;

    requestIdRef.current = requestId;

    if (showLoading) {
      setLoadState({ status: "loading", workspaceId: targetWorkspaceId, tabId: targetTabId });
    }

    try {
      const [snapshot, gitStatusSnapshot] = await Promise.all([
        getFileTree({ workspaceId: targetWorkspaceId, tabId: targetTabId }),
        getFileTreeGitStatus({ workspaceId: targetWorkspaceId, tabId: targetTabId }).catch(() => ({
          entries: [],
        })),
      ]);
      const gitStatus = gitStatusSnapshot.entries.map((entry) => ({
        path: entry.path,
        status: pierreGitStatus(entry.status),
      }));

      if (requestIdRef.current === requestId) {
        const signature = JSON.stringify({ snapshot, gitStatus });
        if (!forceUpdate && snapshotSignatureRef.current === signature) {
          return;
        }

        snapshotSignatureRef.current = signature;
        revisionRef.current += 1;
        setLoadState({
          status: "loaded",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          snapshot,
          gitStatus,
          revision: revisionRef.current,
        });
      }
    } catch (caughtError: unknown) {
      if (requestIdRef.current === requestId) {
        snapshotSignatureRef.current = null;
        setLoadState({
          status: "error",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          message: errorMessage(caughtError),
        });
      }
    }
  };

  const loadWorkspaceRevision = async () => {
    if (revisionLoadInFlightRef.current) {
      revisionLoadPendingRef.current = true;
      return;
    }

    revisionLoadInFlightRef.current = true;
    try {
      do {
        revisionLoadPendingRef.current = false;
        const target = revisionLoadTargetRef.current;
        await loadFileTree(target.workspaceId, target.tabId, false, false);
      } while (revisionLoadPendingRef.current);
    } finally {
      revisionLoadInFlightRef.current = false;
    }
  };

  useEffect(() => {
    observedWorkspaceRevisionRef.current = workspaceRevision;
    snapshotSignatureRef.current = null;
    void loadFileTree(workspaceId, tabId, true, true);
  }, [workspaceId, tabId]);

  useEffect(() => {
    if (workspaceRevision === observedWorkspaceRevisionRef.current) {
      return;
    }

    observedWorkspaceRevisionRef.current = workspaceRevision;
    void loadWorkspaceRevision();
  }, [workspaceRevision, workspaceId, tabId]);

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
          gitStatus={currentLoadState.gitStatus}
          onReload={() => loadFileTree(workspaceId, tabId, false, true)}
        />
      ) : null}
    </div>
  );
}

function LoadedFileTree({
  workspaceId,
  tabId,
  snapshot,
  gitStatus,
  onReload,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  snapshot: FileTreeSnapshot;
  gitStatus: GitStatusEntry[];
  onReload(): Promise<void>;
}) {
  const openEditorTab = useWorkspaceStore((state) => state.openEditorTab);
  const [clipboard, setClipboard] = useState<FileTreeClipboard | null>(null);
  const [rootContextMenuPosition, setRootContextMenuPosition] =
    useState<RootContextMenuPosition | null>(null);
  const [hasInlineCreate, setHasInlineCreate] = useState(false);
  const pendingCreatesRef = useRef<Map<string, PendingFileTreeCreate>>(new Map());
  const closeRowContextMenuRef = useRef<FileTreeContextMenuClose | null>(null);
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
      canDrag: (paths) => paths.length > 0 && !paths.includes(snapshot.rootPath),
      canDrop: (event) => event.target.kind === "root" || event.target.directoryPath !== null,
      onDropComplete: moveDroppedEntries,
      onDropError: (message) => window.alert(message),
    },
    density: "compact",
    flattenEmptyDirectories: false,
    gitStatus,
    initialExpandedPaths: snapshot.expandedPaths,
    initialExpansion: "closed",
    paths: snapshot.paths,
    renaming: {
      canRename: (item) => item.path !== snapshot.rootPath,
      onError: (message) => {
        removePendingCreates();
        window.alert(message);
      },
      onRename: renameEntry,
    },
    stickyFolders: true,
    unsafeCSS: rootActionPaddingCss(snapshot.rootPath),
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
  const registerRowContextMenu = (close: FileTreeContextMenuClose) => {
    closeRootContextMenu();
    closeRowContextMenuRef.current = close;

    return () => {
      if (closeRowContextMenuRef.current === close) {
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
    const targetParentPath = parentPath ?? snapshot.rootPath;
    const placeholderPath = nextUntitledPath(model, targetParentPath, kind);
    const sourcePath = renameSourcePath(placeholderPath);

    pendingCreatesRef.current.set(sourcePath, { kind });
    setHasInlineCreate(true);

    const parent = model.getItem(targetParentPath);
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
  const openClickedFile = (event: MouseEvent<HTMLElement>) => {
    if (event.ctrlKey || event.metaKey || event.shiftKey) {
      return;
    }

    const path = treeItemPathFromEvent(event);
    const item = path ? model.getItem(path) : null;

    if (!path || !item || item.isDirectory() || isPendingCreatePath(path)) {
      return;
    }

    const relativePath = workspaceRelativeTreePath(snapshot.rootPath, path);
    if (relativePath) {
      openEditorTab(tabId, relativePath);
    }
  };

  return (
    <div className="relative h-full min-h-0 overflow-hidden" onContextMenu={openRootContextMenu}>
      <PierreFileTree
        model={model}
        onClick={openClickedFile}
        className="h-full min-h-0 w-full overflow-hidden bg-background text-foreground [--trees-accent-override:var(--accent)] [--trees-bg-muted-override:var(--accent)] [--trees-bg-override:var(--background)] [--trees-border-color-override:var(--border)] [--trees-fg-muted-override:var(--muted-foreground)] [--trees-fg-override:var(--foreground)] [--trees-focus-ring-color-override:var(--ring)] [--trees-input-bg-override:var(--input)] [--trees-item-row-gap-override:6px] [--trees-padding-inline-override:0px] [--trees-scrollbar-gutter-override:0px] [--trees-search-bg-override:var(--input)] [--trees-search-fg-override:var(--foreground)] [--trees-selected-bg-override:var(--accent)] [--trees-selected-fg-override:var(--accent-foreground)] [--trees-selected-focused-border-color-override:var(--ring)]"
        style={{ height: "100%" }}
        renderContextMenu={(item, context) => (
          <FileTreeContextMenu
            clipboard={clipboard}
            context={context}
            item={item}
            model={model}
            rootPath={snapshot.rootPath}
            tabId={tabId}
            workspaceId={workspaceId}
            onClipboardChange={setClipboard}
            onCreateInline={startInlineCreate}
            onIsPendingCreate={isPendingCreatePath}
            onRegisterContextMenu={registerRowContextMenu}
            onRemovePendingCreate={removePendingCreate}
            onMutation={runMutation}
          />
        )}
      />
      <FileTreeRootActions
        model={model}
        rootPath={snapshot.rootPath}
        onCreateInline={startInlineCreate}
        onReload={onReload}
      />
      {snapshot.paths.length === 1 && !hasInlineCreate ? (
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

function FileTreeRootActions({
  model,
  rootPath,
  onCreateInline,
  onReload,
}: {
  model: FileTreeModel;
  rootPath: string;
  onCreateInline(kind: FileTreeEntryKind, parentPath: string | null): void;
  onReload(): Promise<void>;
}) {
  const [isReloading, setIsReloading] = useState(false);
  const createEntry = (kind: FileTreeEntryKind) => {
    onCreateInline(kind, selectedCreationParentPath(model, rootPath));
  };
  const reload = async () => {
    if (isReloading) {
      return;
    }

    setIsReloading(true);
    try {
      await onReload();
    } finally {
      setIsReloading(false);
    }
  };

  return (
    <TooltipProvider>
      <div className="absolute right-1 top-0 z-10 flex h-6 items-center gap-0">
        <FileTreeRootAction label="New file" onClick={() => createEntry("file")}>
          <FilePlus2 />
        </FileTreeRootAction>
        <FileTreeRootAction label="New folder" onClick={() => createEntry("directory")}>
          <FolderPlus />
        </FileTreeRootAction>
        <FileTreeRootAction label="Reload" disabled={isReloading} onClick={() => void reload()}>
          <RefreshCw className={isReloading ? "animate-spin" : undefined} />
        </FileTreeRootAction>
      </div>
    </TooltipProvider>
  );
}

function FileTreeRootAction({
  children,
  disabled,
  label,
  onClick,
}: {
  children: ReactNode;
  disabled?: boolean;
  label: string;
  onClick(): void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="inline-flex" />}>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          disabled={disabled}
          aria-label={label}
          onClick={onClick}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
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
  return (
    <FileTreeContextMenuSurface
      anchorRect={contextMenuAnchorRectFromPoint(position)}
      onCloseComplete={() => onClose()}
    >
      {(close) => {
        const closeAndRun = (mutation: () => Promise<unknown>) => {
          close();
          void onMutation(mutation);
        };

        return (
          <>
            <ContextMenuItem
              onClick={() => {
                close();
                onCreateInline("file", null);
              }}
            >
              New file
            </ContextMenuItem>
            <ContextMenuItem
              onClick={() => {
                close();
                onCreateInline("directory", null);
              }}
            >
              New folder
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem
              disabled={!clipboard}
              onClick={() => {
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
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem
              onClick={() => {
                close();
                void revealFileTreePath({ workspaceId, tabId }).catch((caughtError: unknown) =>
                  window.alert(errorMessage(caughtError)),
                );
              }}
            >
              Reveal in file manager
            </ContextMenuItem>
          </>
        );
      }}
    </FileTreeContextMenuSurface>
  );
}

function FileTreeContextMenu({
  clipboard,
  context,
  item,
  model,
  rootPath,
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
  rootPath: string;
  tabId: TabId;
  workspaceId: WorkspaceId;
  onClipboardChange(clipboard: FileTreeClipboard | null): void;
  onCreateInline(kind: FileTreeEntryKind, parentPath: string | null): void;
  onIsPendingCreate(path: string): boolean;
  onRegisterContextMenu(close: FileTreeContextMenuClose): () => void;
  onRemovePendingCreate(path: string): boolean;
  onMutation(mutation: () => Promise<unknown>): Promise<void>;
}) {
  const itemTargetDirectoryPath = targetDirectoryPathForItem(item);
  const itemIsRoot = item.path === rootPath;
  const itemIsPendingCreate = onIsPendingCreate(item.path);
  const selectedPaths = selectedOrItemPaths(model, item.path);
  const selectedCount = selectedPaths.length;
  const selectedPendingPaths = selectedPaths.filter(onIsPendingCreate);
  const selectedRealPaths = selectedPaths.filter((path) => !onIsPendingCreate(path));
  const hasPendingSelection = selectedPendingPaths.length > 0;
  const hasRootSelection = selectedPaths.includes(rootPath);

  return (
    <FileTreeContextMenuSurface
      anchorRect={context.anchorRect}
      onCloseComplete={(options) => context.close(options)}
      onRegisterClose={onRegisterContextMenu}
    >
      {(close) => {
        const closeAndRun = (mutation: () => Promise<unknown>) => {
          close();
          void onMutation(mutation);
        };

        return (
          <>
            <ContextMenuItem
              onClick={() => {
                close({ restoreFocus: false });
                onCreateInline("file", itemTargetDirectoryPath);
              }}
            >
              New file
            </ContextMenuItem>
            <ContextMenuItem
              onClick={() => {
                close({ restoreFocus: false });
                onCreateInline("directory", itemTargetDirectoryPath);
              }}
            >
              New folder
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem
              disabled={selectedCount !== 1 || itemIsRoot}
              onClick={() => {
                close({ restoreFocus: false });
                model.startRenaming(item.path);
              }}
            >
              Rename
            </ContextMenuItem>
            <ContextMenuItem
              disabled={hasPendingSelection || hasRootSelection}
              onClick={() => {
                onClipboardChange({ action: "copy", paths: selectedPaths });
                close();
              }}
            >
              {selectedCount === 1 ? "Copy" : `Copy ${selectedCount} items`}
            </ContextMenuItem>
            <ContextMenuItem
              disabled={hasPendingSelection || hasRootSelection}
              onClick={() => {
                onClipboardChange({ action: "cut", paths: selectedPaths });
                close();
              }}
            >
              {selectedCount === 1 ? "Cut" : `Cut ${selectedCount} items`}
            </ContextMenuItem>
            <ContextMenuItem
              disabled={!clipboard}
              onClick={() => {
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
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem
              disabled={itemIsPendingCreate}
              onClick={() => {
                close();
                void revealFileTreePath({ workspaceId, tabId, path: item.path }).catch(
                  (caughtError: unknown) => window.alert(errorMessage(caughtError)),
                );
              }}
            >
              Reveal in file manager
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem
              variant="destructive"
              disabled={hasRootSelection}
              onClick={() => {
                if (selectedRealPaths.length === 0) {
                  close();
                  selectedPendingPaths.forEach(onRemovePendingCreate);
                  return;
                }

                const deleteCount = selectedRealPaths.length + selectedPendingPaths.length;
                const message =
                  deleteCount === 1 ? `Delete ${item.name}?` : `Delete ${deleteCount} items?`;

                if (!window.confirm(message)) {
                  close();
                  return;
                }

                closeAndRun(async () => {
                  selectedPendingPaths.forEach(onRemovePendingCreate);
                  await deleteFileTreeEntries({ workspaceId, tabId, paths: selectedRealPaths });
                });
              }}
            >
              {selectedCount === 1 ? "Delete" : `Delete ${selectedCount} items`}
            </ContextMenuItem>
          </>
        );
      }}
    </FileTreeContextMenuSurface>
  );
}

function FileTreeContextMenuSurface({
  anchorRect,
  children,
  onCloseComplete,
  onRegisterClose,
}: {
  anchorRect: FileTreeContextMenuAnchorRect;
  children(close: FileTreeContextMenuClose): ReactNode;
  onCloseComplete(options: FileTreeContextMenuCloseOptions | undefined): void;
  onRegisterClose?: (close: FileTreeContextMenuClose) => () => void;
}) {
  const [open, setOpen] = useState(true);
  const closeOptionsRef = useRef<FileTreeContextMenuCloseOptions | undefined>(undefined);
  const closeRef = useRef<FileTreeContextMenuClose>(() => undefined);
  const didCloseRef = useRef(false);
  const close: FileTreeContextMenuClose = (options) => {
    closeOptionsRef.current = options;
    setOpen(false);
  };

  closeRef.current = close;

  useEffect(() => {
    if (!onRegisterClose) {
      return undefined;
    }

    return onRegisterClose((options) => closeRef.current(options));
  }, []);

  const finishClose = () => {
    if (didCloseRef.current) {
      return;
    }

    didCloseRef.current = true;
    onCloseComplete(closeOptionsRef.current);
  };

  return (
    <ContextMenu
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          close();
        }
      }}
      onOpenChangeComplete={(nextOpen) => {
        if (!nextOpen) {
          finishClose();
        }
      }}
    >
      <ContextMenuContent
        data-file-tree-context-menu-root="true"
        anchor={virtualContextMenuAnchor(anchorRect)}
        alignOffset={0}
        className="min-w-40"
        positionMethod="fixed"
        positionerClassName="z-[1000]"
        sideOffset={0}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
        }}
        onPointerDown={(event) => {
          event.stopPropagation();
        }}
      >
        {children(close)}
      </ContextMenuContent>
    </ContextMenu>
  );
}

function contextMenuAnchorRectFromPoint(position: RootContextMenuPosition): FileTreeContextMenuAnchorRect {
  return {
    top: position.y,
    right: position.x,
    bottom: position.y,
    left: position.x,
    width: 0,
    height: 0,
    x: position.x,
    y: position.y,
  };
}

function virtualContextMenuAnchor(anchorRect: FileTreeContextMenuAnchorRect) {
  return {
    getBoundingClientRect: () =>
      DOMRect.fromRect({
        x: anchorRect.x,
        y: anchorRect.y,
        width: anchorRect.width,
        height: anchorRect.height,
      }),
  };
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

function selectedCreationParentPath(model: FileTreeModel, rootPath: string): string {
  const focusedPath = model.getFocusedPath();
  const selectedPaths = model.getSelectedPaths();
  const selectedPath =
    focusedPath && selectedPaths.includes(focusedPath) ? focusedPath : selectedPaths[0];
  const selectedItem = selectedPath ? model.getItem(selectedPath) : null;

  if (!selectedPath || !selectedItem) {
    return rootPath;
  }

  const parentPath = selectedItem.isDirectory()
    ? targetDirectoryPath(selectedPath)
    : parentDirectoryPath(selectedPath);

  return parentPath ?? rootPath;
}

function workspaceRelativeTreePath(rootPath: string, path: string): string | null {
  const relativePath = path.startsWith(rootPath) ? path.slice(rootPath.length) : "";

  return relativePath.length > 0 ? relativePath : null;
}

function rootActionPaddingCss(rootPath: string): string {
  return `[data-type="item"][data-item-path=${cssString(rootPath)}]{padding-inline-end:76px}`;
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

function treeItemPathFromEvent(event: MouseEvent<HTMLElement>): string | null {
  for (const entry of event.nativeEvent.composedPath()) {
    if (entry instanceof HTMLElement && entry.dataset.itemPath) {
      return entry.dataset.itemPath;
    }
  }

  return null;
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
