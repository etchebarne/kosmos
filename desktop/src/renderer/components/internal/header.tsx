import { Minus, Plus, Square, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import {
  activateWorkspace,
  closeWorkspace,
  closeWindow,
  listWorkspaces,
  minimizeWindow,
  openWorkspace,
  selectWorkspaceDirectory,
  toggleMaximizeWindow,
  type WorkspaceListSnapshot,
} from "@/renderer/ipc";
import { Button } from "@/renderer/components/ui/button";
import { ButtonGroup } from "@/renderer/components/ui/button-group";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/renderer/components/ui/context-menu";
import type { WorkspaceId } from "@/shared/ipc";

export function Header() {
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

  return (
    <header className="relative flex h-8 shrink-0 items-center justify-center px-2 [-webkit-app-region:drag]">
      {error ? (
        <p
          className="absolute right-24 max-w-72 truncate text-xs text-destructive [-webkit-app-region:no-drag]"
          title={error}
        >
          {error}
        </p>
      ) : null}

      <ButtonGroup className="max-w-[min(60vw,42rem)] overflow-hidden [-webkit-app-region:no-drag]">
        {isLoadingWorkspaces ? (
          <Button type="button" variant="outline" size="sm" disabled>
            Loading workspaces
          </Button>
        ) : (
          snapshot?.workspaces.map((workspace) => {
            const isActive = workspace.id === snapshot.activeWorkspaceId;

            return (
              <ContextMenu key={workspace.id}>
                <ContextMenuTrigger
                  render={
                    <Button
                      type="button"
                      variant={isActive ? "default" : "outline"}
                      size="sm"
                      aria-pressed={isActive}
                      onClick={() => void switchWorkspace(workspace.id)}
                    >
                      <span className="max-w-36 truncate">{workspace.name}</span>
                    </Button>
                  }
                />
                <ContextMenuContent>
                  <ContextMenuItem
                    variant="destructive"
                    onClick={() => void closeWorkspaceFromMenu(workspace.id)}
                  >
                    Close workspace
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
            );
          })
        )}

        <Button
          type="button"
          variant="outline"
          size="icon-sm"
          aria-label="Open workspace"
          title="Open workspace"
          disabled={isLoadingWorkspaces || isAddingWorkspace}
          onClick={() => void addWorkspace()}
        >
          <Plus />
        </Button>
      </ButtonGroup>

      <div className="absolute right-1 flex items-center gap-0.5 [-webkit-app-region:no-drag]">
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Minimize window"
          title="Minimize"
          onClick={() => void minimizeWindow()}
        >
          <Minus className="size-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Maximize window"
          title="Maximize"
          onClick={() => void toggleMaximizeWindow()}
        >
          <Square className="size-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="hover:bg-destructive/15 hover:text-destructive"
          aria-label="Close window"
          title="Close"
          onClick={() => void closeWindow()}
        >
          <X className="size-3.5" />
        </Button>
      </div>
    </header>
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

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return "Unable to communicate with the Kosmos server.";
}
