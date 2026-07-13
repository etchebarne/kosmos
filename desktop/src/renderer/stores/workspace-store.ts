import { create } from "zustand";

import { errorMessage } from "@/renderer/lib/errors";
import {
  disposeEditorBuffer,
  disposeAllEditorBuffers,
  disposeWorkspaceEditorBuffers,
  flushEditorBuffer,
  flushEditorBuffers,
  flushWorkspaceEditorBuffers,
  editorBuffer,
} from "@/renderer/lib/editor-buffers";
import {
  activeWorkspaceFrom,
  mergeLocalSplitRatios,
  moveWorkspaceLocally,
  resizeSplitLocally,
} from "@/renderer/lib/workspace-snapshot";
import { canConsumeRequest, createRequestGeneration } from "@/renderer/lib/request-generation";
import {
  activatePane as activatePaneIpc,
  activateTab as activateTabIpc,
  activateWorkspace,
  closeApplication,
  closeTab as closeTabIpc,
  closeWorkspace as closeWorkspaceIpc,
  listWorkspaces,
  moveWorkspace as moveWorkspaceIpc,
  moveTab as moveTabIpc,
  openEditorLocation as openEditorLocationIpc,
  openEditorTab as openEditorTabIpc,
  openGitDiffTab as openGitDiffTabIpc,
  openTab as openTabIpc,
  openWorkspace,
  resizeSplit as resizeSplitIpc,
  resolveTabClose,
  resolveWorkspaceClose,
  setFileTreeExpandedPaths as setFileTreeExpandedPathsIpc,
  setTabKind as setTabKindIpc,
  selectWorkspaceDirectory,
  splitTab as splitTabIpc,
  type WorkspaceListSnapshot,
} from "@/renderer/ipc";
import type {
  CloseDocumentDecision,
  CloseResult,
  UnsavedDocument,
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
type PendingClose = {
  closeId: number;
  documents: UnsavedDocument[];
  scope: "tab" | "workspace" | "application";
  tabId?: TabId;
  workspaceId?: WorkspaceId;
};
type CloseResolution = "cancel" | Record<string, CloseDocumentDecision>;

function resolvedDocumentDecision(
  resolution: Record<string, CloseDocumentDecision>,
  document: Pick<UnsavedDocument, "workspaceId" | "tabId">,
): CloseDocumentDecision {
  const decision = resolution[`${document.workspaceId}:${document.tabId}`];
  if (!decision) {
    throw new Error("Choose a save or discard decision for every changed document.");
  }
  return decision;
}

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
    tabId: TabId | null;
    workspaceId: WorkspaceId;
    path: string;
    lineNumber: number;
    column: number;
    endLineNumber: number;
    endColumn: number;
  } | null;
  pendingClose: PendingClose | null;
  activatePane(paneId: PaneId): void;
  activateTab(paneId: PaneId, tabId: TabId): void;
  addWorkspace(): Promise<void>;
  closeTab(paneId: PaneId, tabId: TabId): void;
  closeWorkspace(workspaceId: WorkspaceId): Promise<void>;
  flushPendingState(): Promise<void>;
  initializeWorkspaces(): Promise<void>;
  refreshWorkspaces(): Promise<void>;
  resetPendingCloseAfterServerRestart(): void;
  requestApplicationClose(): Promise<boolean>;
  resolvePendingClose(resolution: CloseResolution): Promise<void>;
  moveTab(paneId: PaneId, tabId: TabId, targetPaneId: PaneId, targetIndex: number): void;
  moveWorkspace(workspaceId: WorkspaceId, targetIndex: number): void;
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
  const workspaceMoveRequests = createRequestGeneration();
  let navigationQueue = Promise.resolve();
  let applicationCloseResolver: ((completed: boolean) => void) | null = null;

  function closeDocumentKey(document: Pick<UnsavedDocument, "workspaceId" | "tabId">): string {
    return `${document.workspaceId}:${document.tabId}`;
  }

  function disposeClosedTarget(pending: PendingClose): void {
    if (pending.scope === "application") {
      disposeAllEditorBuffers();
      set({ dirtyTabs: {} });
      return;
    }
    if (pending.scope === "tab" && pending.tabId !== undefined && pending.workspaceId !== undefined) {
      disposeEditorBuffer(pending.workspaceId, pending.tabId);
      get().setTabDirty(pending.workspaceId, pending.tabId, false);
      return;
    }
    if (pending.workspaceId === undefined) return;
    const workspaceId = pending.workspaceId;
    disposeWorkspaceEditorBuffers(workspaceId);
    set((state) => {
      if (!state.dirtyTabs[workspaceId]) return {};
      const dirtyTabs = { ...state.dirtyTabs };
      delete dirtyTabs[workspaceId];
      return { dirtyTabs };
    });
  }

  function setCloseResult(pending: Omit<PendingClose, "closeId" | "documents">, result: CloseResult): void {
    if (result.status === "requiresDocumentDecision") {
      set({
        pendingClose: {
          ...pending,
          closeId: result.closeId,
          documents: result.documents,
        },
      });
      return;
    }
    disposeClosedTarget({ ...pending, closeId: 0, documents: [] });
    set({ snapshot: result.snapshot, pendingClose: null });
  }

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
    pendingClose: null,
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

      void (async () => {
        try {
          const buffer = editorBuffer(activeWorkspace.id, tabId);
          if (buffer) await flushEditorBuffer(buffer);
          const result = await closeTabIpc({ workspaceId: activeWorkspace.id, paneId, tabId });
          setCloseResult(
            { scope: "tab", workspaceId: activeWorkspace.id, tabId },
            result,
          );
        } catch (caughtError) {
          set({ error: errorMessage(caughtError) });
        }
      })();
    },
    async closeWorkspace(workspaceId) {
      try {
        await flushWorkspaceEditorBuffers(workspaceId);
        const result = await closeWorkspaceIpc(workspaceId);
        setCloseResult({ scope: "workspace", workspaceId }, result);
      } catch (caughtError) {
        set({ error: errorMessage(caughtError) });
      }
    },
    flushPendingState() {
      return Promise.all([flushFileTreeExpansionSaves(), flushEditorBuffers()]).then(() => undefined);
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
    resetPendingCloseAfterServerRestart() {
      if (!get().pendingClose) {
        return;
      }
      applicationCloseResolver?.(false);
      applicationCloseResolver = null;
      set({
        pendingClose: null,
        error: "The Kosmos server restarted. Review unsaved changes and try closing again.",
      });
    },
    async requestApplicationClose() {
      try {
        await get().flushPendingState();
        const result = await closeApplication();
        if (result.status === "completed") {
          disposeAllEditorBuffers();
          set({ dirtyTabs: {}, snapshot: result.snapshot, pendingClose: null });
          return true;
        }
        set({
          pendingClose: {
            scope: "application",
            closeId: result.closeId,
            documents: result.documents,
          },
        });
        return await new Promise<boolean>((resolve) => {
          applicationCloseResolver = resolve;
        });
      } catch (caughtError) {
        set({ error: errorMessage(caughtError) });
        throw caughtError;
      }
    },
    async resolvePendingClose(resolution) {
      const pending = get().pendingClose;
      if (!pending) return;
      try {
        const result =
          resolution === "cancel"
            ? pending.scope === "tab"
              ? await resolveTabClose({ closeId: pending.closeId, decision: { kind: "cancel" } })
              : await resolveWorkspaceClose({ closeId: pending.closeId, decision: { kind: "cancel" } })
            : pending.scope === "tab"
              ? await resolveTabClose({
                  closeId: pending.closeId,
                  decision: {
                    kind: "resolve",
                    documents: pending.documents.map((document) => ({
                      workspaceId: document.workspaceId,
                      tabId: document.tabId,
                      revision: document.revision,
                      decision: resolvedDocumentDecision(resolution, document),
                    })),
                  },
                })
              : await resolveWorkspaceClose({
                  closeId: pending.closeId,
                  decision: {
                    kind: "resolve",
                    documents: pending.documents.map((document) => ({
                      workspaceId: document.workspaceId,
                      tabId: document.tabId,
                      revision: document.revision,
                      decision: resolvedDocumentDecision(resolution, document),
                    })),
                  },
                });
        if (result.status !== "completed") {
          set({
            pendingClose: {
              ...pending,
              closeId: result.closeId,
              documents: result.documents,
            },
          });
          return;
        }
        if (resolution !== "cancel") disposeClosedTarget(pending);
        set({ snapshot: result.snapshot, pendingClose: null });
        if (pending.scope === "application") {
          applicationCloseResolver?.(resolution !== "cancel");
          applicationCloseResolver = null;
        }
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
    moveWorkspace(workspaceId, targetIndex) {
      const previousSnapshot = get().snapshot;
      const snapshot = moveWorkspaceLocally(previousSnapshot, workspaceId, targetIndex);
      if (snapshot === previousSnapshot) {
        return;
      }

      const generation = workspaceMoveRequests.issue();
      set({ error: null, snapshot });

      void moveWorkspaceIpc({ workspaceId, targetIndex })
        .then((nextSnapshot) => {
          if (!workspaceMoveRequests.isCurrent(generation)) {
            return;
          }

          set((state) => ({
            snapshot: mergeLocalSplitRatios(nextSnapshot, state.snapshot),
          }));
        })
        .catch(async (caughtError: unknown) => {
          if (!workspaceMoveRequests.isCurrent(generation)) {
            return;
          }

          let fallbackSnapshot = previousSnapshot;
          try {
            fallbackSnapshot = await listWorkspaces();
          } catch {
            // Keep the previous local order when the server cannot be reached.
          }

          if (workspaceMoveRequests.isCurrent(generation)) {
            set((state) => ({
              error: errorMessage(caughtError),
              snapshot: fallbackSnapshot
                ? mergeLocalSplitRatios(fallbackSnapshot, state.snapshot)
                : fallbackSnapshot,
            }));
          }
        });
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
      const generation = navigationRequests.issue();
      set({
        error: null,
        pendingEditorSelection: {
          generation,
          tabId: null,
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
          const result = await openEditorLocationIpc({ workspaceId, path });
          if (!navigationRequests.isCurrent(generation)) {
            return;
          }
          set((state) => ({
            snapshot: result.snapshot,
            pendingEditorSelection:
              state.pendingEditorSelection?.generation === generation
                ? {
                    ...state.pendingEditorSelection,
                    tabId: result.target.tabId,
                    workspaceId: result.target.workspaceId,
                    path: result.target.path,
                  }
                : state.pendingEditorSelection,
          }));
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
