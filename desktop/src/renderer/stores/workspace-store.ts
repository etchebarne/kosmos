import { create } from "zustand";

import { errorMessage } from "@/renderer/lib/errors";
import {
  disposeEditorBuffer,
  disposeWorkspaceEditorBuffers,
} from "@/renderer/lib/editor-buffers";
import {
  activeWorkspaceFrom,
  closeWorkspaceLocally,
  editorSourceTabId,
  mergeLocalSplitRatios,
  resizeSplitLocally,
} from "@/renderer/lib/workspace-snapshot";
import { canConsumeRequest, createRequestGeneration } from "@/renderer/lib/request-generation";
import {
  activatePane as activatePaneIpc,
  activateTab as activateTabIpc,
  activateWorkspace,
  closeTab as closeTabIpc,
  closeWorkspace as closeWorkspaceIpc,
  listWorkspaces,
  moveTab as moveTabIpc,
  openEditorTab as openEditorTabIpc,
  openGitDiffTab as openGitDiffTabIpc,
  openTab as openTabIpc,
  openWorkspace,
  resizeSplit as resizeSplitIpc,
  setFileTreeExpandedPaths as setFileTreeExpandedPathsIpc,
  setTabKind as setTabKindIpc,
  selectWorkspaceDirectory,
  splitTab as splitTabIpc,
  type WorkspaceListSnapshot,
} from "@/renderer/ipc";
import type {
  OpenableTabKind,
  PaneId,
  SplitAxis,
  SplitPaneId,
  SetFileTreeExpandedPathsParams,
  TabId,
  WorkspaceId,
} from "@/shared/ipc";

import { useGitStore } from "./git-store";

type WorkspaceRequest = () => Promise<WorkspaceListSnapshot>;
type FileTreeExpansionFlusher = () => Promise<void> | void;

type WorkspaceStore = {
  dirtyTabs: Record<WorkspaceId, Record<TabId, true>>;
  error: string | null;
  isAddingWorkspace: boolean;
  isLoadingWorkspaces: boolean;
  loadRequestId: number;
  snapshot: WorkspaceListSnapshot | null;
  switchRequestId: number;
  pendingEditorSelection: {
    generation: number;
    workspaceId: WorkspaceId;
    path: string;
    lineNumber: number;
    column: number;
    endLineNumber: number;
    endColumn: number;
  } | null;
  activatePane(paneId: PaneId): void;
  activateTab(paneId: PaneId, tabId: TabId): void;
  addWorkspace(): Promise<void>;
  closeTab(paneId: PaneId, tabId: TabId): void;
  closeWorkspace(workspaceId: WorkspaceId): Promise<void>;
  flushPendingState(): Promise<void>;
  initializeWorkspaces(): Promise<void>;
  refreshWorkspaces(): Promise<void>;
  moveTab(paneId: PaneId, tabId: TabId, targetPaneId: PaneId, targetIndex: number): void;
  openEditorTab(tabId: TabId, path: string): void;
  openEditorLocation(
    workspaceId: WorkspaceId,
    path: string,
    lineNumber: number,
    column: number,
    endLineNumber?: number,
    endColumn?: number,
  ): Promise<boolean>;
  consumePendingEditorSelection(generation: number): boolean;
  openGitDiffTab(tabId: TabId, path: string): void;
  openTab(paneId: PaneId): void;
  registerFileTreeExpansionFlusher(flusher: FileTreeExpansionFlusher): () => void;
  resizeSplit(splitId: SplitPaneId, ratio: number): void;
  saveFileTreeExpandedPaths(params: SetFileTreeExpandedPathsParams): Promise<void>;
  setTabDirty(workspaceId: WorkspaceId, tabId: TabId, isDirty: boolean): void;
  setTabKind(paneId: PaneId, tabId: TabId, kind: OpenableTabKind): void;
  splitTab(
    paneId: PaneId,
    tabId: TabId,
    targetPaneId: PaneId,
    axis: SplitAxis,
    newPaneFirst: boolean,
  ): void;
  switchWorkspace(workspaceId: WorkspaceId): Promise<void>;
};

export const useWorkspaceStore = create<WorkspaceStore>((set, get) => {
  const fileTreeExpansionFlushers = new Set<FileTreeExpansionFlusher>();
  const pendingFileTreeExpansionSaves = new Set<Promise<void>>();
  let pendingResizeRequests = 0;
  let resizeFallbackSnapshot: WorkspaceListSnapshot | null = null;
  let resizeNeedsReconciliation = false;
  let resizeRequestId = 0;
  const navigationRequests = createRequestGeneration();
  let navigationQueue = Promise.resolve();

  function updateFromServer(request: WorkspaceRequest): void {
    set({ error: null });

    void request()
      .then((nextSnapshot) => {
        set((state) => ({
          snapshot: mergeLocalSplitRatios(nextSnapshot, state.snapshot),
        }));
      })
      .catch((caughtError: unknown) => {
        set({ error: errorMessage(caughtError) });
      });
  }

  function trackFileTreeExpansionSave(promise: Promise<void>): Promise<void> {
    pendingFileTreeExpansionSaves.add(promise);

    void promise.finally(() => {
      pendingFileTreeExpansionSaves.delete(promise);
    });

    return promise;
  }

  async function flushFileTreeExpansionSaves(): Promise<void> {
    const flushPromises: Promise<void>[] = [];

    for (const flusher of fileTreeExpansionFlushers) {
      try {
        flushPromises.push(
          Promise.resolve(flusher()).catch((caughtError: unknown) => {
            set({ error: errorMessage(caughtError) });
          }),
        );
      } catch (caughtError) {
        set({ error: errorMessage(caughtError) });
      }
    }

    await Promise.all(flushPromises);

    while (pendingFileTreeExpansionSaves.size > 0) {
      await Promise.all(Array.from(pendingFileTreeExpansionSaves));
    }
  }

  return {
    dirtyTabs: {},
    error: null,
    isAddingWorkspace: false,
    isLoadingWorkspaces: true,
    loadRequestId: 0,
    snapshot: null,
    switchRequestId: 0,
    pendingEditorSelection: null,
    activatePane(paneId) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace || paneId === activeWorkspace.activePaneId) {
        return;
      }

      updateFromServer(() => activatePaneIpc({ workspaceId: activeWorkspace.id, paneId }));
    },
    activateTab(paneId, tabId) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() => activateTabIpc({ workspaceId: activeWorkspace.id, paneId, tabId }));
    },
    async addWorkspace() {
      set({ error: null, isAddingWorkspace: true });

      try {
        const directory = await selectWorkspaceDirectory();

        if (!directory) {
          return;
        }

        set({ snapshot: await openWorkspace(directory) });
      } catch (caughtError) {
        set({ error: errorMessage(caughtError) });
      } finally {
        set({ isAddingWorkspace: false });
      }
    },
    closeTab(paneId, tabId) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        closeTabIpc({ workspaceId: activeWorkspace.id, paneId, tabId }).then((snapshot) => {
          window.setTimeout(() => disposeEditorBuffer(activeWorkspace.id, tabId), 0);
          get().setTabDirty(activeWorkspace.id, tabId, false);
          return snapshot;
        }),
      );
    },
    async closeWorkspace(workspaceId) {
      const { snapshot, switchRequestId } = get();
      const requestId = switchRequestId + 1;
      const previousSnapshot = snapshot;

      set({
        error: null,
        snapshot: closeWorkspaceLocally(snapshot, workspaceId),
        switchRequestId: requestId,
      });

      try {
        const nextSnapshot = await closeWorkspaceIpc(workspaceId);
        disposeWorkspaceEditorBuffers(workspaceId);
        set((state) => {
          if (!state.dirtyTabs[workspaceId]) {
            return {};
          }

          const dirtyTabs = { ...state.dirtyTabs };
          delete dirtyTabs[workspaceId];

          return { dirtyTabs };
        });

        if (get().switchRequestId === requestId) {
          set({ snapshot: nextSnapshot });
        }
      } catch (caughtError) {
        if (get().switchRequestId === requestId) {
          set({ error: errorMessage(caughtError), snapshot: previousSnapshot });
        }
      }
    },
    flushPendingState() {
      return flushFileTreeExpansionSaves();
    },
    async initializeWorkspaces() {
      const requestId = get().loadRequestId + 1;

      set({ isLoadingWorkspaces: true, loadRequestId: requestId });

      try {
        const snapshot = await listWorkspaces();

        if (get().loadRequestId === requestId) {
          set({ error: null, snapshot });
        }
      } catch (caughtError) {
        if (get().loadRequestId === requestId) {
          set({ error: errorMessage(caughtError) });
        }
      } finally {
        if (get().loadRequestId === requestId) {
          set({ isLoadingWorkspaces: false });
        }
      }
    },
    async refreshWorkspaces() {
      try {
        const snapshot = await listWorkspaces();
        set((state) => ({
          error: null,
          snapshot: mergeLocalSplitRatios(snapshot, state.snapshot),
        }));
      } catch (caughtError) {
        set({ error: errorMessage(caughtError) });
      }
    },
    moveTab(paneId, tabId, targetPaneId, targetIndex) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        moveTabIpc({ workspaceId: activeWorkspace.id, paneId, tabId, targetPaneId, targetIndex }),
      );
    },
    openEditorTab(tabId, path) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        openEditorTabIpc({ workspaceId: activeWorkspace.id, tabId, path }),
      );
    },
    async openEditorLocation(
      workspaceId,
      path,
      lineNumber,
      column,
      endLineNumber = lineNumber,
      endColumn = column,
    ) {
      const workspace = get().snapshot?.workspaces.find((candidate) => candidate.id === workspaceId);
      if (!workspace) {
        set({ error: "The requested workspace is not open." });
        return false;
      }
      const sourceTabId = editorSourceTabId(workspace.root);
      if (sourceTabId === null) {
        set({ error: "Open a File Tree or Search tab before navigating to a language location." });
        return false;
      }
      const generation = navigationRequests.issue();
      set({
        error: null,
        pendingEditorSelection: {
          generation,
          workspaceId,
          path,
          lineNumber,
          column,
          endLineNumber,
          endColumn,
        },
      });

      let succeeded = false;
      const navigation = navigationQueue.then(async () => {
        if (!navigationRequests.isCurrent(generation)) {
          return;
        }
        try {
          const activeSnapshot = await activateWorkspace(workspaceId);
          if (!navigationRequests.isCurrent(generation)) {
            return;
          }
          set({ snapshot: activeSnapshot });
          const snapshot = await openEditorTabIpc({ workspaceId, tabId: sourceTabId, path });
          if (!navigationRequests.isCurrent(generation)) {
            return;
          }
          set({ snapshot });
          succeeded = true;
        } catch (caughtError) {
          if (navigationRequests.isCurrent(generation)) {
            set((state) => ({
              error: errorMessage(caughtError),
              pendingEditorSelection:
                state.pendingEditorSelection?.generation === generation
                  ? null
                  : state.pendingEditorSelection,
            }));
          }
        }
      });
      navigationQueue = navigation.catch(() => {});
      await navigation;
      return succeeded;
    },
    consumePendingEditorSelection(generation) {
      if (!canConsumeRequest(get().pendingEditorSelection?.generation ?? null, generation)) {
        return false;
      }
      set({ pendingEditorSelection: null });
      return true;
    },
    openGitDiffTab(tabId, path) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        openGitDiffTabIpc({ workspaceId: activeWorkspace.id, tabId, path }).then((snapshot) => {
          useGitStore.getState().bumpGitRevision(activeWorkspace.id);
          return snapshot;
        }),
      );
    },
    openTab(paneId) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        openTabIpc({ workspaceId: activeWorkspace.id, paneId, kind: "blank" }),
      );
    },
    registerFileTreeExpansionFlusher(flusher) {
      fileTreeExpansionFlushers.add(flusher);

      return () => {
        fileTreeExpansionFlushers.delete(flusher);
      };
    },
    resizeSplit(splitId, ratio) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }
      const previousSnapshot = get().snapshot;

      set((state) => ({
        error: null,
        snapshot: resizeSplitLocally(state.snapshot, activeWorkspace.id, splitId, ratio),
      }));
      if (pendingResizeRequests === 0) {
        resizeFallbackSnapshot = previousSnapshot;
      }
      pendingResizeRequests += 1;
      const requestId = ++resizeRequestId;

      void resizeSplitIpc({ workspaceId: activeWorkspace.id, splitId, ratio })
        .then((snapshot) => {
          resizeFallbackSnapshot = snapshot;

          if (resizeRequestId === requestId || resizeNeedsReconciliation) {
            set({ snapshot });
          }
        })
        .catch(async (caughtError: unknown) => {
          if (resizeRequestId !== requestId) {
            return;
          }

          for (let attempt = 0; attempt < 5; attempt += 1) {
            if (attempt > 0) {
              await new Promise((resolve) => setTimeout(resolve, attempt * 50));
            }

            try {
              const snapshot = await listWorkspaces();
              resizeFallbackSnapshot = snapshot;

              if (resizeRequestId === requestId) {
                resizeNeedsReconciliation = false;
                set({ error: errorMessage(caughtError), snapshot });
              }
              return;
            } catch {
              // Retry after the ordered request queue has had time to drain.
            }
          }

          resizeNeedsReconciliation = true;
          set({ error: errorMessage(caughtError), snapshot: resizeFallbackSnapshot });
        })
        .finally(() => {
          pendingResizeRequests -= 1;

          if (pendingResizeRequests === 0) {
            resizeFallbackSnapshot = null;
            resizeNeedsReconciliation = false;
          }
        });
    },
    saveFileTreeExpandedPaths(params) {
      const save = setFileTreeExpandedPathsIpc(params)
        .then(() => undefined)
        .catch((caughtError: unknown) => {
          set({ error: errorMessage(caughtError) });
        });

      return trackFileTreeExpansionSave(save);
    },
    setTabDirty(workspaceId, tabId, isDirty) {
      set((state) => {
        const workspaceTabs = state.dirtyTabs[workspaceId] ?? {};
        const wasDirty = workspaceTabs[tabId] === true;

        if (wasDirty === isDirty) {
          return {};
        }

        const dirtyTabs = { ...state.dirtyTabs };

        if (isDirty) {
          dirtyTabs[workspaceId] = { ...workspaceTabs, [tabId]: true };
        } else {
          const { [tabId]: _removed, ...remainingTabs } = workspaceTabs;

          if (Object.keys(remainingTabs).length === 0) {
            delete dirtyTabs[workspaceId];
          } else {
            dirtyTabs[workspaceId] = remainingTabs;
          }
        }

        return { dirtyTabs };
      });
    },
    setTabKind(paneId, tabId, kind) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        setTabKindIpc({ workspaceId: activeWorkspace.id, paneId, tabId, kind }),
      );
    },
    splitTab(paneId, tabId, targetPaneId, axis, newPaneFirst) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        splitTabIpc({
          workspaceId: activeWorkspace.id,
          paneId,
          targetPaneId,
          tabId,
          axis,
          newPaneFirst,
        }),
      );
    },
    async switchWorkspace(workspaceId) {
      const { isAddingWorkspace, snapshot, switchRequestId } = get();
      if (workspaceId === snapshot?.activeWorkspaceId || isAddingWorkspace) {
        return;
      }

      navigationRequests.invalidate();
      const requestId = switchRequestId + 1;
      const previousSnapshot = snapshot;

      set({ error: null, pendingEditorSelection: null, switchRequestId: requestId });

      await flushFileTreeExpansionSaves();

      if (get().switchRequestId !== requestId) {
        return;
      }

      set({
        error: null,
        snapshot: snapshot ? { ...snapshot, activeWorkspaceId: workspaceId } : snapshot,
      });

      try {
        const nextSnapshot = await activateWorkspace(workspaceId);

        if (get().switchRequestId === requestId) {
          set((state) => ({
            snapshot: mergeLocalSplitRatios(nextSnapshot, state.snapshot),
          }));
        }
      } catch (caughtError) {
        if (get().switchRequestId === requestId) {
          set({ error: errorMessage(caughtError), snapshot: previousSnapshot });
        }
      }
    },
  };
});
