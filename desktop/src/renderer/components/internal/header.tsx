import { Menu, Minus, Plus, Settings, Square, X } from "lucide-react";
import { useState } from "react";

import {
  closeWindow,
  minimizeWindow,
  toggleMaximizeWindow,
} from "@/renderer/ipc";
import { useWorkspaceStore } from "@/renderer/stores";
import { Button } from "@/renderer/components/ui/button";
import { ButtonGroup } from "@/renderer/components/ui/button-group";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/renderer/components/ui/dropdown-menu";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/renderer/components/ui/context-menu";
import { SettingsDialog } from "./settings-dialog";

export function Header() {
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const addWorkspace = useWorkspaceStore((state) => state.addWorkspace);
  const closeWorkspace = useWorkspaceStore((state) => state.closeWorkspace);
  const error = useWorkspaceStore((state) => state.error);
  const isAddingWorkspace = useWorkspaceStore((state) => state.isAddingWorkspace);
  const isLoadingWorkspaces = useWorkspaceStore((state) => state.isLoadingWorkspaces);
  const snapshot = useWorkspaceStore((state) => state.snapshot);
  const switchWorkspace = useWorkspaceStore((state) => state.switchWorkspace);

  return (
    <header className="relative flex h-8 shrink-0 items-center justify-center px-2 [-webkit-app-region:drag]">
      <div className="absolute left-1 flex items-center [-webkit-app-region:no-drag]">
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="ghost" size="icon-xs" aria-label="Open application menu" />
            }
          >
            <Menu className="size-3.5" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-44">
            <DropdownMenuItem onClick={() => setIsSettingsOpen(true)}>
              <Settings />
              Settings
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

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
                    onClick={() => void closeWorkspace(workspace.id)}
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
          onClick={() => void minimizeWindow()}
        >
          <Minus className="size-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Maximize window"
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
          onClick={() => void closeWindow()}
        >
          <X className="size-3.5" />
        </Button>
      </div>
      <SettingsDialog open={isSettingsOpen} onOpenChange={setIsSettingsOpen} />
    </header>
  );
}
