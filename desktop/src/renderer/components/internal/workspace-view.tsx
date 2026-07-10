import { useState, type DragEvent, type WheelEvent } from "react";
import { Plus, X } from "lucide-react";

import { FileIcon, FileIconSprite } from "@/renderer/components/file-icon";
import { Button } from "@/renderer/components/ui/button";
import { TabErrorBoundary } from "@/renderer/components/tabs/error-boundary";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/renderer/components/ui/context-menu";
import { renderTabContent, tabKindIcon } from "@/renderer/components/tabs";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/renderer/components/ui/resizable";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/renderer/components/ui/tabs";
import { cn } from "@/renderer/lib/utils";
import { useWorkspaceStore } from "@/renderer/stores";
import type {
  PaneId,
  PaneNodeSnapshot,
  PaneSnapshot,
  SplitAxis,
  TabId,
  TabSnapshot,
  WorkspaceId,
} from "@/shared/ipc";

type DropEdge = "left" | "right" | "top" | "bottom";

type DraggedTab = {
  paneId: PaneId;
  tabId: TabId;
};

type TabDropTarget = {
  index: number;
  x: number;
};

const TAB_DRAG_MIME = "application/x-kosmos-tab";
const TAB_TRIGGER_SELECTOR = "[data-kosmos-tab-trigger]";
const MIN_PANE_SIZE_REM = 16;

export function WorkspaceView() {
  const addWorkspace = useWorkspaceStore((state) => state.addWorkspace);
  const isAddingWorkspace = useWorkspaceStore((state) => state.isAddingWorkspace);
  const isLoadingWorkspaces = useWorkspaceStore((state) => state.isLoadingWorkspaces);
  const snapshot = useWorkspaceStore((state) => state.snapshot);
  const activeWorkspaceId = snapshot?.activeWorkspaceId ?? null;

  if (isLoadingWorkspaces) {
    return (
      <section className="grid min-h-0 flex-1 place-items-center overflow-hidden rounded-2xl border bg-background text-center shadow-sm">
        <p className="text-sm text-muted-foreground">Loading workspace state...</p>
      </section>
    );
  }

  if (
    !activeWorkspaceId ||
    !snapshot?.workspaces.some((workspace) => workspace.id === activeWorkspaceId)
  ) {
    return (
      <section className="grid min-h-0 flex-1 place-items-center overflow-hidden rounded-2xl border bg-background p-8 text-center shadow-sm">
        <div className="flex max-w-sm flex-col items-center gap-4">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight">Welcome to Kosmos</h1>
            <p className="mt-2 text-sm text-muted-foreground">
              Open a workspace to start exploring your project.
            </p>
          </div>
          <Button type="button" disabled={isAddingWorkspace} onClick={() => void addWorkspace()}>
            <Plus />
            Open workspace
          </Button>
        </div>
      </section>
    );
  }

  return (
    <section className="flex min-h-0 min-w-0 flex-1 overflow-hidden rounded-2xl border bg-background shadow-sm">
      <FileIconSprite />
      {snapshot.workspaces.map((workspace) => {
        const isWorkspaceActive = workspace.id === activeWorkspaceId;

        return (
          <div key={workspace.id} hidden={!isWorkspaceActive} className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
            <PaneNodeView
              node={workspace.root}
              workspaceId={workspace.id}
              isWorkspaceActive={isWorkspaceActive}
              isRoot
            />
          </div>
        );
      })}
    </section>
  );
}

function PaneNodeView({
  node,
  workspaceId,
  isWorkspaceActive,
  isRoot = false,
}: {
  node: PaneNodeSnapshot;
  workspaceId: WorkspaceId;
  isWorkspaceActive: boolean;
  isRoot?: boolean;
}) {
  const resizeSplit = useWorkspaceStore((state) => state.resizeSplit);

  if (node.type === "leaf") {
    return (
      <PaneLeaf
        pane={node.pane}
        workspaceId={workspaceId}
        isWorkspaceActive={isWorkspaceActive}
        isRoot={isRoot}
      />
    );
  }

  const firstPanelId = `split-${node.id}-first`;
  const secondPanelId = `split-${node.id}-second`;
  const firstSize = ratioToPercent(node.ratio);
  const secondSize = 100 - firstSize;

  return (
    <ResizablePanelGroup
      key={`${workspaceId}:${node.id}`}
      id={`workspace-${workspaceId}-split-${node.id}`}
      orientation={node.axis}
      defaultLayout={{
        [firstPanelId]: firstSize,
        [secondPanelId]: secondSize,
      }}
      className={cn("min-h-0 min-w-0 overflow-hidden", isRoot && "flex-1")}
      onLayoutChanged={(layout, meta) => {
        if (!meta.isUserInteraction) {
          return;
        }

        const firstLayoutSize = layout[firstPanelId];
        const secondLayoutSize = layout[secondPanelId];
        if (firstLayoutSize === undefined || secondLayoutSize === undefined) {
          return;
        }

        const nextRatio = firstLayoutSize / (firstLayoutSize + secondLayoutSize);
        if (!Number.isFinite(nextRatio) || nextRatio === node.ratio) {
          return;
        }

        resizeSplit(node.id, nextRatio);
      }}
    >
      <ResizablePanel
        id={firstPanelId}
        defaultSize={percentSize(firstSize)}
        minSize={minimumNodeSize(node.first, node.axis)}
      >
        <PaneNodeView
          node={node.first}
          workspaceId={workspaceId}
          isWorkspaceActive={isWorkspaceActive}
        />
      </ResizablePanel>
      <ResizableHandle withHandle />
      <ResizablePanel
        id={secondPanelId}
        defaultSize={percentSize(secondSize)}
        minSize={minimumNodeSize(node.second, node.axis)}
      >
        <PaneNodeView
          node={node.second}
          workspaceId={workspaceId}
          isWorkspaceActive={isWorkspaceActive}
        />
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}

function PaneLeaf({
  pane,
  workspaceId,
  isWorkspaceActive,
  isRoot,
}: {
  pane: PaneSnapshot;
  workspaceId: WorkspaceId;
  isWorkspaceActive: boolean;
  isRoot: boolean;
}) {
  const [dropEdge, setDropEdge] = useState<DropEdge | null>(null);
  const [tabDropTarget, setTabDropTarget] = useState<TabDropTarget | null>(null);
  const activatePane = useWorkspaceStore((state) => state.activatePane);
  const activateTab = useWorkspaceStore((state) => state.activateTab);
  const moveTab = useWorkspaceStore((state) => state.moveTab);
  const openTab = useWorkspaceStore((state) => state.openTab);
  const splitTab = useWorkspaceStore((state) => state.splitTab);

  return (
    <article
      className={cn(
        "relative flex min-h-0 min-w-0 flex-col overflow-hidden bg-card text-card-foreground",
        isRoot ? "flex-1" : "h-full",
      )}
      onDragLeave={(event) => {
        const relatedTarget = event.relatedTarget;
        if (relatedTarget instanceof Node && event.currentTarget.contains(relatedTarget)) {
          return;
        }

        setDropEdge(null);
        setTabDropTarget(null);
      }}
      onDragOver={(event) => {
        if (!hasDraggedTab(event)) {
          return;
        }

        setTabDropTarget(null);
        const edge = edgeFromDragEvent(event, event.currentTarget);
        setDropEdge(edge);

        if (!edge) {
          event.dataTransfer.dropEffect = "none";
          return;
        }

        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
      }}
      onDrop={(event) => {
        const draggedTab = readDraggedTab(event);
        const edge = edgeFromDragEvent(event, event.currentTarget);
        setDropEdge(null);
        setTabDropTarget(null);

        if (!draggedTab || !edge) {
          return;
        }

        const split = splitDetailsFromEdge(edge);
        event.preventDefault();
        splitTab(
          draggedTab.paneId,
          draggedTab.tabId,
          pane.id,
          split.axis,
          split.newPaneFirst,
        );
      }}
    >
      <Tabs
        value={tabValue(pane.activeTabId)}
        className="h-full min-h-0 min-w-0 gap-0"
        onValueChange={(value) => activateTabFromValue(value, pane, activateTab)}
      >
        <div
          className="relative flex h-10 shrink-0 items-center gap-1 border-b bg-muted/60 px-1"
          onDragOver={(event) => {
            if (!hasDraggedTab(event)) {
              return;
            }

            const target = tabDropTargetFromDragEvent(event, event.currentTarget);
            setDropEdge(null);
            setTabDropTarget(target);
            event.preventDefault();
            event.stopPropagation();
            event.dataTransfer.dropEffect = "move";
          }}
          onDrop={(event) => {
            const draggedTab = readDraggedTab(event);
            if (!draggedTab) {
              return;
            }

            const target = tabDropTargetFromDragEvent(event, event.currentTarget);
            setDropEdge(null);
            setTabDropTarget(null);
            event.preventDefault();
            event.stopPropagation();

            if (!isTabDropNoop(pane, draggedTab, target.index)) {
              moveTab(draggedTab.paneId, draggedTab.tabId, pane.id, target.index);
            }
          }}
        >
          <div
            className="scrollbar-none flex h-full min-w-0 flex-1 items-center overflow-x-auto overflow-y-hidden overscroll-x-contain"
            onWheel={scrollTabStripOnWheel}
          >
            <TabsList variant="line" className="h-8 w-max min-w-full justify-start rounded-none p-0">
              {pane.tabs.map((tab) => (
                <TabTrigger key={tab.id} pane={pane} tab={tab} workspaceId={workspaceId} />
              ))}
            </TabsList>
          </div>

          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="shrink-0"
            aria-label="Open tab"
            onClick={() => openTab(pane.id)}
          >
            <Plus />
          </Button>

          <TabDropIndicator target={tabDropTarget} />
        </div>

        {pane.tabs.map((tab) => {
          const isTabActive = isWorkspaceActive && tab.id === pane.activeTabId;
          const shouldRenderTabBody = isWorkspaceActive || tab.lifecycle === "keepAlive";

          return (
            <TabsContent
              key={tab.id}
              value={tabValue(tab.id)}
              keepMounted={tab.lifecycle === "keepAlive"}
              className="min-h-0 min-w-0 overflow-hidden p-0"
            >
              {shouldRenderTabBody ? (
                <TabBody
                  paneId={pane.id}
                  tab={tab}
                  workspaceId={workspaceId}
                  isActive={isTabActive}
                  onActivatePane={() => activatePane(pane.id)}
                />
              ) : null}
            </TabsContent>
          );
        })}
      </Tabs>

      <DropIndicator edge={dropEdge} />
    </article>
  );
}

function TabTrigger({
  pane,
  tab,
  workspaceId,
}: {
  pane: PaneSnapshot;
  tab: TabSnapshot;
  workspaceId: WorkspaceId;
}) {
  const TabIcon = tabKindIcon(tab.kind);
  const closeTab = useWorkspaceStore((state) => state.closeTab);
  const isTabDirty = useWorkspaceStore(
    (state) => state.dirtyTabs[workspaceId]?.[tab.id] === true,
  );

  return (
    <ContextMenu>
      <ContextMenuTrigger
        render={
          <TabsTrigger
            value={tabValue(tab.id)}
            nativeButton={false}
            draggable
            render={<div />}
            data-kosmos-tab-trigger=""
            className="group/tab max-w-52 flex-none cursor-default justify-start px-2 text-xs after:hidden data-active:!bg-foreground/10 data-active:!text-foreground"
            aria-label={isTabDirty ? `${tab.title}, unsaved changes` : tab.title}
            onDragStart={(event) => writeDraggedTab(event, pane.id, tab.id, tab.title)}
            onPointerDown={(event) => {
              if (event.button !== 1) {
                return;
              }

              event.preventDefault();
              event.stopPropagation();
            }}
            onAuxClick={(event) => {
              if (event.button !== 1) {
                return;
              }

              event.preventDefault();
              event.stopPropagation();
              closeTab(pane.id, tab.id);
            }}
          >
            {tab.kind === "editor" ? (
              <FileIcon
                path={tab.title}
                className="size-3.5 shrink-0 text-muted-foreground"
              />
            ) : (
              <TabIcon
                className="size-3.5 shrink-0 text-muted-foreground group-data-active/tab:text-foreground"
                aria-hidden="true"
              />
            )}
            <span className="truncate">{tab.title}</span>
            <button
              type="button"
              draggable={false}
              className={cn(
                "group/close relative ml-1 -mr-1 grid size-5 shrink-0 place-items-center rounded text-muted-foreground transition-opacity hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                isTabDirty
                  ? "opacity-100"
                  : "opacity-0 group-data-active/tab:opacity-60 group-hover/tab:opacity-60 focus-visible:opacity-100",
              )}
              aria-label={`Close ${tab.title}`}
              onPointerDown={(event) => {
                event.preventDefault();
                event.stopPropagation();
              }}
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                closeTab(pane.id, tab.id);
              }}
            >
              <X
                className={cn(
                  "absolute size-3 transition-opacity",
                  isTabDirty &&
                    "opacity-0 group-hover/tab:opacity-100 group-focus-visible/close:opacity-100",
                )}
              />
              {isTabDirty ? (
                <span
                  aria-hidden="true"
                  title="Unsaved changes"
                  className="absolute size-1.5 rounded-full bg-foreground transition-opacity group-hover/tab:opacity-0 group-focus-visible/close:opacity-0"
                />
              ) : null}
            </button>
          </TabsTrigger>
        }
      />
      <ContextMenuContent>
        <ContextMenuItem variant="destructive" onClick={() => closeTab(pane.id, tab.id)}>
          Close tab
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function TabBody({
  paneId,
  tab,
  workspaceId,
  isActive,
  onActivatePane,
}: {
  paneId: PaneId;
  tab: TabSnapshot;
  workspaceId: WorkspaceId;
  isActive: boolean;
  onActivatePane(): void;
}) {
  const setTabKind = useWorkspaceStore((state) => state.setTabKind);

  return (
    <TabErrorBoundary key={`${tab.id}:${tab.kind}`}>
      {renderTabContent({
        paneId,
        tab,
        workspaceId,
        isActive,
        onActivatePane,
        onSetTabKind: (kind) => setTabKind(paneId, tab.id, kind),
      })}
    </TabErrorBoundary>
  );
}

function TabDropIndicator({ target }: { target: TabDropTarget | null }) {
  if (!target) {
    return null;
  }

  return (
    <div
      className="pointer-events-none absolute top-1.5 bottom-1.5 z-30 w-0.5 -translate-x-1/2 rounded-full bg-primary ring-4 ring-primary/15"
      style={{ left: target.x }}
    />
  );
}

function DropIndicator({ edge }: { edge: DropEdge | null }) {
  if (!edge) {
    return null;
  }

  return (
    <div className="pointer-events-none absolute inset-0 z-20 rounded-xl bg-primary/5 ring-2 ring-primary/25">
      <div
        className={cn(
          "absolute rounded-lg bg-primary/15 ring-1 ring-primary/50",
          edge === "left" && "top-2 bottom-2 left-2 w-1/3",
          edge === "right" && "top-2 right-2 bottom-2 w-1/3",
          edge === "top" && "top-2 right-2 left-2 h-1/3",
          edge === "bottom" && "right-2 bottom-2 left-2 h-1/3",
        )}
      />
    </div>
  );
}

function activateTabFromValue(
  value: unknown,
  pane: PaneSnapshot,
  activateTab: (paneId: PaneId, tabId: TabId) => void,
): void {
  const tabId = Number(value);

  if (!Number.isSafeInteger(tabId) || tabId === pane.activeTabId) {
    return;
  }

  if (!pane.tabs.some((tab) => tab.id === tabId)) {
    return;
  }

  activateTab(pane.id, tabId);
}

function writeDraggedTab(
  event: DragEvent<HTMLElement>,
  paneId: PaneId,
  tabId: TabId,
  title: string,
): void {
  event.dataTransfer.effectAllowed = "move";
  event.dataTransfer.setData(TAB_DRAG_MIME, JSON.stringify({ paneId, tabId }));
  event.dataTransfer.setData("text/plain", title);
}

function readDraggedTab(event: DragEvent<HTMLElement>): DraggedTab | null {
  const payload = event.dataTransfer.getData(TAB_DRAG_MIME);
  if (!payload) {
    return null;
  }

  let value: unknown;
  try {
    value = JSON.parse(payload);
  } catch {
    return null;
  }

  if (!value || typeof value !== "object") {
    return null;
  }

  const { paneId, tabId } = value as Partial<DraggedTab>;
  if (
    typeof paneId !== "number" ||
    typeof tabId !== "number" ||
    !Number.isSafeInteger(paneId) ||
    !Number.isSafeInteger(tabId)
  ) {
    return null;
  }

  return { paneId, tabId };
}

function hasDraggedTab(event: DragEvent<HTMLElement>): boolean {
  for (let index = 0; index < event.dataTransfer.types.length; index += 1) {
    if (event.dataTransfer.types[index] === TAB_DRAG_MIME) {
      return true;
    }
  }

  return false;
}

function scrollTabStripOnWheel(event: WheelEvent<HTMLDivElement>): void {
  const scroller = event.currentTarget;
  const maxScrollLeft = scroller.scrollWidth - scroller.clientWidth;

  if (maxScrollLeft <= 0 || Math.abs(event.deltaX) >= Math.abs(event.deltaY)) {
    return;
  }

  const nextScrollLeft = clamp(scroller.scrollLeft + event.deltaY, 0, maxScrollLeft);
  if (nextScrollLeft === scroller.scrollLeft) {
    return;
  }

  scroller.scrollLeft = nextScrollLeft;
  event.preventDefault();
}

function tabDropTargetFromDragEvent(
  event: DragEvent<HTMLElement>,
  element: HTMLElement,
): TabDropTarget {
  const containerRect = element.getBoundingClientRect();
  const tabElements = Array.from(element.querySelectorAll<HTMLElement>(TAB_TRIGGER_SELECTOR));

  if (tabElements.length === 0) {
    return {
      index: 0,
      x: clamp(event.clientX - containerRect.left, 0, containerRect.width),
    };
  }

  for (let index = 0; index < tabElements.length; index += 1) {
    const tabRect = tabElements[index]!.getBoundingClientRect();
    if (event.clientX < tabRect.left + tabRect.width / 2) {
      return {
        index,
        x: clamp(tabRect.left - containerRect.left, 0, containerRect.width),
      };
    }
  }

  const lastTabRect = tabElements[tabElements.length - 1]!.getBoundingClientRect();
  return {
    index: tabElements.length,
    x: clamp(lastTabRect.right - containerRect.left, 0, containerRect.width),
  };
}

function isTabDropNoop(pane: PaneSnapshot, draggedTab: DraggedTab, targetIndex: number): boolean {
  if (draggedTab.paneId !== pane.id) {
    return false;
  }

  const currentIndex = pane.tabs.findIndex((tab) => tab.id === draggedTab.tabId);
  return currentIndex === -1 || targetIndex === currentIndex || targetIndex === currentIndex + 1;
}

function edgeFromDragEvent(event: DragEvent<HTMLElement>, element: HTMLElement): DropEdge | null {
  const rect = element.getBoundingClientRect();
  const threshold = Math.min(96, Math.max(40, Math.min(rect.width, rect.height) * 0.28));
  const distances: Array<[DropEdge, number]> = [
    ["left", event.clientX - rect.left],
    ["right", rect.right - event.clientX],
    ["top", event.clientY - rect.top],
    ["bottom", rect.bottom - event.clientY],
  ];
  const [edge, distance] = distances.reduce((closest, current) =>
    current[1] < closest[1] ? current : closest,
  );

  return distance <= threshold ? edge : null;
}

function splitDetailsFromEdge(edge: DropEdge): { axis: SplitAxis; newPaneFirst: boolean } {
  return {
    axis: edge === "left" || edge === "right" ? "horizontal" : "vertical",
    newPaneFirst: edge === "left" || edge === "top",
  };
}

function tabValue(tabId: TabId): string {
  return String(tabId);
}

function ratioToPercent(ratio: number): number {
  return Math.min(Math.max(ratio * 100, 1), 99);
}

function percentSize(value: number): string {
  return `${value}%`;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function minimumNodeSize(node: PaneNodeSnapshot, axis: SplitAxis): string {
  return `${minimumNodeSizeRem(node, axis)}rem`;
}

function minimumNodeSizeRem(node: PaneNodeSnapshot, axis: SplitAxis): number {
  if (node.type === "leaf") {
    return MIN_PANE_SIZE_REM;
  }

  const firstMinimum = minimumNodeSizeRem(node.first, axis);
  const secondMinimum = minimumNodeSizeRem(node.second, axis);

  return node.axis === axis
    ? firstMinimum + secondMinimum
    : Math.max(firstMinimum, secondMinimum);
}
