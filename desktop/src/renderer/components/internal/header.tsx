import { Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import {
  activateWorkspace,
  listWorkspaces,
  openWorkspace,
  selectWorkspaceDirectory,
  type WorkspaceListSnapshot,
} from "@/renderer/ipc";
import { Button } from "@/renderer/components/ui/button";
import { ButtonGroup } from "@/renderer/components/ui/button-group";
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

  return (
    <header className="relative flex h-12 shrink-0 items-center justify-center px-4">
      {error ? (
        <p className="absolute right-4 max-w-72 truncate text-xs text-destructive" title={error}>
          {error}
        </p>
      ) : null}

      <ButtonGroup className="max-w-[min(60vw,42rem)] overflow-hidden">
        {isLoadingWorkspaces ? (
          <Button type="button" variant="outline" size="sm" disabled>
            Loading workspaces
          </Button>
        ) : (
          snapshot?.workspaces.map((workspace) => {
            const isActive = workspace.id === snapshot.activeWorkspaceId;

            return (
              <Button
                key={workspace.id}
                type="button"
                variant={isActive ? "default" : "outline"}
                size="sm"
                aria-pressed={isActive}
                title={workspace.directory}
                onClick={() => void switchWorkspace(workspace.id)}
              >
                <span className="max-w-36 truncate">{workspace.name}</span>
              </Button>
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
    </header>
  );
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return "Unable to communicate with the Kosmos server.";
}
