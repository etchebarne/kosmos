import { useEffect, useRef, useState } from "react";

import { Header } from "@/renderer/components/internal/header";
import {
  WorkspaceView,
  type WorkspaceViewActions,
} from "@/renderer/components/internal/workspace-view";
import { errorMessage } from "@/renderer/lib/errors";
import {
  activatePane,
  activateTab,
  activateWorkspace,
  closeTab,
  closeWorkspace,
  listWorkspaces,
  openTab,
  openWorkspace,
  resizeSplit,
  setTabKind,
  selectWorkspaceDirectory,
  splitTab,
  type WorkspaceListSnapshot,
} from "@/renderer/ipc";
import type {
  PaneId,
  SplitAxis,
  SplitPaneId,
  TabId,
  TabKind,
  WorkspaceId,
  WorkspaceSnapshot,
} from "@/shared/ipc";

export function App() {
  const [error, setError] = useState<string | null>(null);
  const [isAddingWorkspace, setIsAddingWorkspace] = useState(false);
  const [isLoadingWorkspaces, setIsLoadingWorkspaces] = useState(true);
  const [snapshot, setSnapshot] = useState<WorkspaceListSnapshot | null>(null);
  const switchRequestId = useRef(0);

  useEffect(() => {
    let isMounted = true;

    listWorkspaces()
      .then((nextSnapshot) => {
        if (!isMounted) {
          return;
        }

        setSnapshot(nextSnapshot);
        setError(null);
      })
      .catch((caughtError: unknown) => {
        if (isMounted) {
          setError(errorMessage(caughtError));
        }
      })
      .finally(() => {
        if (isMounted) {
          setIsLoadingWorkspaces(false);
        }
      });

    return () => {
      isMounted = false;
    };
  }, []);

  async function switchWorkspace(workspaceId: WorkspaceId): Promise<void> {
    if (workspaceId === snapshot?.activeWorkspaceId || isAddingWorkspace) {
      return;
    }

    const requestId = switchRequestId.current + 1;
    const previousSnapshot = snapshot;

    switchRequestId.current = requestId;
    setError(null);
    setSnapshot((currentSnapshot) =>
      currentSnapshot ? { ...currentSnapshot, activeWorkspaceId: workspaceId } : currentSnapshot,
    );

    try {
      const nextSnapshot = await activateWorkspace(workspaceId);

      if (switchRequestId.current === requestId) {
        setSnapshot(nextSnapshot);
      }
    } catch (caughtError) {
      if (switchRequestId.current === requestId) {
        setSnapshot(previousSnapshot);
        setError(errorMessage(caughtError));
      }
    }
  }

  async function addWorkspace(): Promise<void> {
    setIsAddingWorkspace(true);
    setError(null);

    try {
      const directory = await selectWorkspaceDirectory();

      if (!directory) {
        return;
      }

      setSnapshot(await openWorkspace(directory));
    } catch (caughtError) {
      setError(errorMessage(caughtError));
    } finally {
      setIsAddingWorkspace(false);
    }
  }

  async function closeWorkspaceFromMenu(workspaceId: WorkspaceId): Promise<void> {
    const requestId = switchRequestId.current + 1;
    const previousSnapshot = snapshot;

    switchRequestId.current = requestId;
    setError(null);
    setSnapshot((currentSnapshot) => closeWorkspaceLocally(currentSnapshot, workspaceId));

    try {
      const nextSnapshot = await closeWorkspace(workspaceId);

      if (switchRequestId.current === requestId) {
        setSnapshot(nextSnapshot);
      }
    } catch (caughtError) {
      if (switchRequestId.current === requestId) {
        setSnapshot(previousSnapshot);
        setError(errorMessage(caughtError));
      }
    }
  }

  function updateFromServer(request: () => Promise<WorkspaceListSnapshot>): void {
    setError(null);

    void request()
      .then((nextSnapshot) => {
        setSnapshot(nextSnapshot);
      })
      .catch((caughtError: unknown) => {
        setError(errorMessage(caughtError));
      });
  }

  const activeWorkspace = activeWorkspaceFrom(snapshot);
  const workspaceActions: WorkspaceViewActions = {
    activatePane(paneId: PaneId) {
      if (!activeWorkspace || paneId === activeWorkspace.activePaneId) {
        return;
      }

      updateFromServer(() => activatePane({ workspaceId: activeWorkspace.id, paneId }));
    },
    activateTab(paneId: PaneId, tabId: TabId) {
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() => activateTab({ workspaceId: activeWorkspace.id, paneId, tabId }));
    },
    closeTab(paneId: PaneId, tabId: TabId) {
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() => closeTab({ workspaceId: activeWorkspace.id, paneId, tabId }));
    },
    openTab(paneId: PaneId) {
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() => openTab({ workspaceId: activeWorkspace.id, paneId, kind: "blank" }));
    },
    resizeSplit(splitId: SplitPaneId, ratio: number) {
      if (!activeWorkspace) {
        return;
      }

      setError(null);
      void resizeSplit({ workspaceId: activeWorkspace.id, splitId, ratio }).catch(
        (caughtError: unknown) => {
          setError(errorMessage(caughtError));
        },
      );
    },
    setTabKind(paneId: PaneId, tabId: TabId, kind: TabKind) {
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        setTabKind({ workspaceId: activeWorkspace.id, paneId, tabId, kind }),
      );
    },
    splitTab(
      paneId: PaneId,
      tabId: TabId,
      targetPaneId: PaneId,
      axis: SplitAxis,
      newPaneFirst: boolean,
    ) {
      if (!activeWorkspace) {
        return;
      }

      updateFromServer(() =>
        splitTab({
          workspaceId: activeWorkspace.id,
          paneId,
          targetPaneId,
          tabId,
          axis,
          newPaneFirst,
        }),
      );
    },
  };

  return (
    <main className="flex h-full flex-col gap-2 overflow-hidden bg-muted text-foreground">
      <Header
        error={error}
        isAddingWorkspace={isAddingWorkspace}
        isLoadingWorkspaces={isLoadingWorkspaces}
        snapshot={snapshot}
        onCloseWorkspace={(workspaceId) => void closeWorkspaceFromMenu(workspaceId)}
        onOpenWorkspace={() => void addWorkspace()}
        onSwitchWorkspace={(workspaceId) => void switchWorkspace(workspaceId)}
      />

      <WorkspaceView
        isAddingWorkspace={isAddingWorkspace}
        isLoading={isLoadingWorkspaces}
        workspace={activeWorkspace}
        actions={workspaceActions}
        onOpenWorkspace={() => void addWorkspace()}
      />
    </main>
  );
}

function activeWorkspaceFrom(snapshot: WorkspaceListSnapshot | null): WorkspaceSnapshot | null {
  if (!snapshot?.activeWorkspaceId) {
    return null;
  }

  return (
    snapshot.workspaces.find((workspace) => workspace.id === snapshot.activeWorkspaceId) ?? null
  );
}

function closeWorkspaceLocally(
  snapshot: WorkspaceListSnapshot | null,
  workspaceId: WorkspaceId,
): WorkspaceListSnapshot | null {
  if (!snapshot) {
    return snapshot;
  }

  const workspaceIndex = snapshot.workspaces.findIndex((workspace) => workspace.id === workspaceId);

  if (workspaceIndex === -1) {
    return snapshot;
  }

  const workspaces = snapshot.workspaces.filter((workspace) => workspace.id !== workspaceId);
  const activeWorkspaceId =
    snapshot.activeWorkspaceId === workspaceId
      ? (workspaces[workspaceIndex]?.id ?? workspaces[workspaceIndex - 1]?.id ?? null)
      : snapshot.activeWorkspaceId;

  return { ...snapshot, activeWorkspaceId, workspaces };
}
