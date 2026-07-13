import { Menu, Minus, Plus, Settings, Square, X } from "lucide-react";
import { useState, type DragEvent } from "react";

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
import type { WorkspaceId, WorkspaceSnapshot } from "@/shared/ipc";
import { SettingsDialog } from "./settings-dialog";

const WORKSPACE_DRAG_MIME = "application/x-kosmos-workspace";
const WORKSPACE_TRIGGER_SELECTOR = "[data-kosmos-workspace-trigger]";

type WorkspaceDropTarget = {
  index: number;
  x: number;
};

export function Header() {
  const [draggedWorkspaceId, setDraggedWorkspaceId] = useState<WorkspaceId | null>(null);
  const [workspaceDropTarget, setWorkspaceDropTarget] = useState<WorkspaceDropTarget | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const addWorkspace = useWorkspaceStore((state) => state.addWorkspace);
  const closeWorkspace = useWorkspaceStore((state) => state.closeWorkspace);
  const error = useWorkspaceStore((state) => state.error);
  const isAddingWorkspace = useWorkspaceStore((state) => state.isAddingWorkspace);
  const isLoadingWorkspaces = useWorkspaceStore((state) => state.isLoadingWorkspaces);
  const moveWorkspace = useWorkspaceStore((state) => state.moveWorkspace);
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

      <ButtonGroup
        className="relative max-w-[min(60vw,42rem)] overflow-hidden [-webkit-app-region:no-drag]"
        onDragLeave={(event) => {
          const relatedTarget = event.relatedTarget;
          if (relatedTarget instanceof Node && event.currentTarget.contains(relatedTarget)) {
            return;
          }

          setWorkspaceDropTarget(null);
        }}
        onDragOver={(event) => {
          if (!hasDraggedWorkspace(event)) {
            return;
          }

          setWorkspaceDropTarget(workspaceDropTargetFromDragEvent(event, event.currentTarget));
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
        }}
        onDrop={(event) => {
          const workspaceId = readDraggedWorkspace(event);
          const target = workspaceDropTargetFromDragEvent(event, event.currentTarget);
          setDraggedWorkspaceId(null);
          setWorkspaceDropTarget(null);

          if (!workspaceId || !snapshot) {
            return;
          }

          event.preventDefault();
          if (!isWorkspaceDropNoop(snapshot.workspaces, workspaceId, target.index)) {
            moveWorkspace(workspaceId, target.index);
          }
        }}
      >
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
                      draggable
                      variant={isActive ? "default" : "outline"}
                      size="sm"
                      data-kosmos-workspace-trigger=""
                      className={
                        draggedWorkspaceId === workspace.id
                          ? "cursor-grabbing opacity-50"
                          : "cursor-grab"
                      }
                      aria-pressed={isActive}
                      onClick={() => void switchWorkspace(workspace.id)}
                      onDragStart={(event) => {
                        setDraggedWorkspaceId(workspace.id);
                        writeDraggedWorkspace(event, workspace.id, workspace.name);
                      }}
                      onDragEnd={() => {
                        setDraggedWorkspaceId(null);
                        setWorkspaceDropTarget(null);
                      }}
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
        <WorkspaceDropIndicator target={workspaceDropTarget} />
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

function WorkspaceDropIndicator({ target }: { target: WorkspaceDropTarget | null }) {
  if (!target) {
    return null;
  }

  return (
    <div
      className="pointer-events-none absolute top-0.5 bottom-0.5 z-30 w-0.5 -translate-x-1/2 rounded-full bg-primary ring-4 ring-primary/15"
      style={{ left: target.x }}
    />
  );
}

function writeDraggedWorkspace(
  event: DragEvent<HTMLElement>,
  workspaceId: WorkspaceId,
  name: string,
): void {
  event.dataTransfer.effectAllowed = "move";
  event.dataTransfer.setData(WORKSPACE_DRAG_MIME, String(workspaceId));
  event.dataTransfer.setData("text/plain", name);
}

function readDraggedWorkspace(event: DragEvent<HTMLElement>): WorkspaceId | null {
  const workspaceId = Number(event.dataTransfer.getData(WORKSPACE_DRAG_MIME));
  return Number.isSafeInteger(workspaceId) && workspaceId > 0 ? workspaceId : null;
}

function hasDraggedWorkspace(event: DragEvent<HTMLElement>): boolean {
  for (let index = 0; index < event.dataTransfer.types.length; index += 1) {
    if (event.dataTransfer.types[index] === WORKSPACE_DRAG_MIME) {
      return true;
    }
  }

  return false;
}

function workspaceDropTargetFromDragEvent(
  event: DragEvent<HTMLElement>,
  element: HTMLElement,
): WorkspaceDropTarget {
  const containerRect = element.getBoundingClientRect();
  const workspaceElements = Array.from(
    element.querySelectorAll<HTMLElement>(WORKSPACE_TRIGGER_SELECTOR),
  );

  for (let index = 0; index < workspaceElements.length; index += 1) {
    const workspaceRect = workspaceElements[index]!.getBoundingClientRect();
    if (event.clientX < workspaceRect.left + workspaceRect.width / 2) {
      return {
        index,
        x: Math.max(0, Math.min(workspaceRect.left - containerRect.left, containerRect.width)),
      };
    }
  }

  const lastWorkspaceRect = workspaceElements.at(-1)?.getBoundingClientRect();
  const x = lastWorkspaceRect ? lastWorkspaceRect.right - containerRect.left : 0;
  return {
    index: workspaceElements.length,
    x: Math.max(0, Math.min(x, containerRect.width)),
  };
}

function isWorkspaceDropNoop(
  workspaces: WorkspaceSnapshot[],
  workspaceId: WorkspaceId,
  targetIndex: number,
): boolean {
  const currentIndex = workspaces.findIndex((workspace) => workspace.id === workspaceId);
  return currentIndex === -1 || targetIndex === currentIndex || targetIndex === currentIndex + 1;
}
