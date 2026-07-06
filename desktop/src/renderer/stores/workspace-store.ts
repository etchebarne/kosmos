import { create } from "zustand";

import { errorMessage } from "@/renderer/lib/errors";
import {
  activeWorkspaceFrom,
  closeWorkspaceLocally,
  mergeLocalSplitRatios,
  resizeSplitLocally,
} from "@/renderer/lib/workspace-snapshot";
import {
  activatePane as activatePaneIpc,
  activateTab as activateTabIpc,
  activateWorkspace,
  closeTab as closeTabIpc,
  closeWorkspace as closeWorkspaceIpc,
  listWorkspaces,
  openTab as openTabIpc,
  openWorkspace,
  resizeSplit as resizeSplitIpc,
  setTabKind as setTabKindIpc,
  selectWorkspaceDirectory,
  splitTab as splitTabIpc,
  type WorkspaceListSnapshot,
} from "@/renderer/ipc";
import type {
  PaneId,
  SplitAxis,
  SplitPaneId,
  TabId,
  TabKind,
  WorkspaceId,
} from "@/shared/ipc";

type WorkspaceRequest = () => Promise<WorkspaceListSnapshot>;

type WorkspaceStore = {
  error: string | null;
  isAddingWorkspace: boolean;
  isLoadingWorkspaces: boolean;
  loadRequestId: number;
  snapshot: WorkspaceListSnapshot | null;
  switchRequestId: number;
  activatePane(paneId: PaneId): void;
  activateTab(paneId: PaneId, tabId: TabId): void;
  addWorkspace(): Promise<void>;
  closeTab(paneId: PaneId, tabId: TabId): void;
  closeWorkspace(workspaceId: WorkspaceId): Promise<void>;
  initializeWorkspaces(): Promise<void>;
  openTab(paneId: PaneId): void;
  resizeSplit(splitId: SplitPaneId, ratio: number): void;
  setTabKind(paneId: PaneId, tabId: TabId, kind: TabKind): void;
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

  return {
    error: null,
    isAddingWorkspace: false,
    isLoadingWorkspaces: true,
    loadRequestId: 0,
    snapshot: null,
    switchRequestId: 0,
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

      updateFromServer(() => closeTabIpc({ workspaceId: activeWorkspace.id, paneId, tabId }));
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

        if (get().switchRequestId === requestId) {
          set({ snapshot: nextSnapshot });
        }
      } catch (caughtError) {
        if (get().switchRequestId === requestId) {
          set({ error: errorMessage(caughtError), snapshot: previousSnapshot });
        }
      }
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
    openTab(paneId) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        openTabIpc({ workspaceId: activeWorkspace.id, paneId, kind: "blank" }),
      );
    },
    resizeSplit(splitId, ratio) {
      const activeWorkspace = activeWorkspaceFrom(get().snapshot);
      if (!activeWorkspace) {
        return;
      }

      set((state) => ({
        error: null,
        snapshot: resizeSplitLocally(state.snapshot, activeWorkspace.id, splitId, ratio),
      }));
      void resizeSplitIpc({ workspaceId: activeWorkspace.id, splitId, ratio }).catch(
        (caughtError: unknown) => {
          set({ error: errorMessage(caughtError) });
        },
      );
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

      const requestId = switchRequestId + 1;
      const previousSnapshot = snapshot;

      set({
        error: null,
        snapshot: snapshot ? { ...snapshot, activeWorkspaceId: workspaceId } : snapshot,
        switchRequestId: requestId,
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
