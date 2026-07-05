import { Minus, Plus, Square, X } from "lucide-react";

import {
  closeWindow,
  minimizeWindow,
  toggleMaximizeWindow,
} from "@/renderer/ipc";
import { Button } from "@/renderer/components/ui/button";
import { ButtonGroup } from "@/renderer/components/ui/button-group";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/renderer/components/ui/context-menu";
import type { WorkspaceId, WorkspaceListSnapshot } from "@/shared/ipc";

type HeaderProps = {
  error: string | null;
  isAddingWorkspace: boolean;
  isLoadingWorkspaces: boolean;
  snapshot: WorkspaceListSnapshot | null;
  onCloseWorkspace(workspaceId: WorkspaceId): void;
  onOpenWorkspace(): void;
  onSwitchWorkspace(workspaceId: WorkspaceId): void;
};

export function Header({
  error,
  isAddingWorkspace,
  isLoadingWorkspaces,
  snapshot,
  onCloseWorkspace,
  onOpenWorkspace,
  onSwitchWorkspace,
}: HeaderProps) {
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
                      onClick={() => onSwitchWorkspace(workspace.id)}
                    >
                      <span className="max-w-36 truncate">{workspace.name}</span>
                    </Button>
                  }
                />
                <ContextMenuContent>
                  <ContextMenuItem
                    variant="destructive"
                    onClick={() => onCloseWorkspace(workspace.id)}
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
          onClick={onOpenWorkspace}
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
