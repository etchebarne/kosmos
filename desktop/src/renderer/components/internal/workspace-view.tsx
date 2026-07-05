import { useState, type DragEvent } from "react";
import {
  File,
  FileText,
  FolderTree,
  GitBranch,
  Plus,
  Search,
  Settings,
  Terminal,
  X,
  type LucideIcon,
} from "lucide-react";

import { Button } from "@/renderer/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/renderer/components/ui/context-menu";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/renderer/components/ui/resizable";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/renderer/components/ui/tabs";
import { cn } from "@/renderer/lib/utils";
import type {
  PaneId,
  PaneNodeSnapshot,
  PaneSnapshot,
  SplitAxis,
  SplitPaneId,
  TabId,
  TabKind,
  TabSnapshot,
  WorkspaceSnapshot,
} from "@/shared/ipc";

export type WorkspaceViewActions = {
  activatePane(paneId: PaneId): void;
  activateTab(paneId: PaneId, tabId: TabId): void;
  closeTab(paneId: PaneId, tabId: TabId): void;
  openTab(paneId: PaneId): void;
  resizeSplit(splitId: SplitPaneId, ratio: number): void;
  splitTab(
    paneId: PaneId,
    tabId: TabId,
    targetPaneId: PaneId,
    axis: SplitAxis,
    newPaneFirst: boolean,
  ): void;
};

type WorkspaceViewProps = {
  actions: WorkspaceViewActions;
  isAddingWorkspace: boolean;
  isLoading: boolean;
  workspace: WorkspaceSnapshot | null;
  onOpenWorkspace(): void;
};

type DropEdge = "left" | "right" | "top" | "bottom";

type DraggedTab = {
  paneId: PaneId;
  tabId: TabId;
};

const TAB_DRAG_MIME = "application/x-kosmos-tab";

const TAB_KIND_LABEL: Record<TabKind, string> = {
  blank: "Blank",
  editor: "Editor",
  fileTree: "File Tree",
  git: "Git",
  search: "Search",
  settings: "Settings",
  terminal: "Terminal",
};

const TAB_KIND_ICON: Record<TabKind, LucideIcon> = {
  blank: File,
  editor: FileText,
  fileTree: FolderTree,
  git: GitBranch,
  search: Search,
  settings: Settings,
  terminal: Terminal,
};

export function WorkspaceView({
  actions,
  isAddingWorkspace,
  isLoading,
  workspace,
  onOpenWorkspace,
}: WorkspaceViewProps) {
  if (isLoading) {
    return (
      <section className="grid min-h-0 flex-1 place-items-center overflow-hidden rounded-2xl border bg-background text-center shadow-sm">
        <p className="text-sm text-muted-foreground">Loading workspace state...</p>
      </section>
    );
  }

  if (!workspace) {
    return (
      <section className="grid min-h-0 flex-1 place-items-center overflow-hidden rounded-2xl border bg-background p-8 text-center shadow-sm">
        <div className="flex max-w-sm flex-col items-center gap-4">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight">No workspace open</h1>
            <p className="mt-2 text-sm text-muted-foreground">
              Open a workspace to create tabs and panes.
            </p>
          </div>
          <Button type="button" disabled={isAddingWorkspace} onClick={onOpenWorkspace}>
            <Plus />
            Open workspace
          </Button>
        </div>
      </section>
    );
  }

  return (
    <section className="flex min-h-0 flex-1 overflow-hidden rounded-2xl border bg-background shadow-sm">
      <PaneNodeView node={workspace.root} workspace={workspace} actions={actions} isRoot />
    </section>
  );
}

function PaneNodeView({
  node,
  workspace,
  actions,
  isRoot = false,
}: {
  node: PaneNodeSnapshot;
  workspace: WorkspaceSnapshot;
  actions: WorkspaceViewActions;
  isRoot?: boolean;
}) {
  if (node.type === "leaf") {
    return <PaneLeaf pane={node.pane} workspace={workspace} actions={actions} isRoot={isRoot} />;
  }

  const firstPanelId = `split-${node.id}-first`;
  const secondPanelId = `split-${node.id}-second`;
  const firstSize = ratioToPercent(node.ratio);
  const secondSize = 100 - firstSize;

  return (
    <ResizablePanelGroup
      key={`${node.id}:${node.ratio.toFixed(4)}`}
      orientation={node.axis}
      className={cn("min-h-0", isRoot && "flex-1")}
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
        if (!Number.isFinite(nextRatio) || Math.abs(nextRatio - node.ratio) < 0.005) {
          return;
        }

        actions.resizeSplit(node.id, nextRatio);
      }}
    >
      <ResizablePanel id={firstPanelId} defaultSize={firstSize} minSize={12}>
        <PaneNodeView node={node.first} workspace={workspace} actions={actions} />
      </ResizablePanel>
      <ResizableHandle withHandle />
      <ResizablePanel id={secondPanelId} defaultSize={secondSize} minSize={12}>
        <PaneNodeView node={node.second} workspace={workspace} actions={actions} />
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}

function PaneLeaf({
  pane,
  workspace,
  actions,
  isRoot,
}: {
  pane: PaneSnapshot;
  workspace: WorkspaceSnapshot;
  actions: WorkspaceViewActions;
  isRoot: boolean;
}) {
  const [dropEdge, setDropEdge] = useState<DropEdge | null>(null);
  return (
    <article
      className={cn(
        "relative flex min-h-0 flex-col overflow-hidden bg-card text-card-foreground",
        isRoot ? "flex-1" : "h-full",
      )}
      onDragLeave={(event) => {
        const relatedTarget = event.relatedTarget;
        if (relatedTarget instanceof Node && event.currentTarget.contains(relatedTarget)) {
          return;
        }

        setDropEdge(null);
      }}
      onDragOver={(event) => {
        if (!hasDraggedTab(event)) {
          return;
        }

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

        if (!draggedTab || !edge) {
          return;
        }

        const split = splitDetailsFromEdge(edge);
        event.preventDefault();
        actions.splitTab(
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
        className="h-full min-h-0 gap-0"
        onValueChange={(value) => activateTabFromValue(value, pane, actions)}
      >
        <div className="flex h-10 shrink-0 items-center gap-1 border-b bg-muted/60 px-2">
          <TabsList
            variant="line"
            className="h-full min-w-0 flex-1 justify-start overflow-x-auto overflow-y-hidden rounded-none p-0 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
          >
            {pane.tabs.map((tab) => (
              <TabTrigger key={tab.id} pane={pane} tab={tab} actions={actions} />
            ))}
          </TabsList>

          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="shrink-0"
            aria-label="Open tab"
            title="Open tab"
            onClick={() => actions.openTab(pane.id)}
          >
            <Plus />
          </Button>
        </div>

        {pane.tabs.map((tab) => (
          <TabsContent key={tab.id} value={tabValue(tab.id)} className="min-h-0 p-0">
            <TabBody tab={tab} onActivatePane={() => actions.activatePane(pane.id)} />
          </TabsContent>
        ))}
      </Tabs>

      <DropIndicator edge={dropEdge} />
    </article>
  );
}

function TabTrigger({
  pane,
  tab,
  actions,
}: {
  pane: PaneSnapshot;
  tab: TabSnapshot;
  actions: WorkspaceViewActions;
}) {
  const TabIcon = TAB_KIND_ICON[tab.kind];

  return (
    <ContextMenu>
      <ContextMenuTrigger
        render={
          <TabsTrigger
            value={tabValue(tab.id)}
            nativeButton={false}
            draggable
            render={<div />}
            className="group/tab max-w-52 flex-none cursor-default justify-start px-2 text-xs"
            title={`${tab.title}. Drag to a pane edge to split.`}
            onDragStart={(event) => writeDraggedTab(event, pane.id, tab.id, tab.title)}
          >
            <TabIcon
              className="size-3.5 shrink-0 text-muted-foreground group-data-active/tab:text-foreground"
              aria-hidden="true"
            />
            <span className="truncate">{tab.title}</span>
            <button
              type="button"
              draggable={false}
              className="ml-1 -mr-1 grid size-5 shrink-0 place-items-center rounded text-muted-foreground opacity-0 transition-opacity hover:bg-muted hover:text-foreground hover:opacity-100 group-data-active/tab:opacity-60 group-hover/tab:opacity-60 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              aria-label={`Close ${tab.title}`}
              title="Close tab"
              onPointerDown={(event) => {
                event.preventDefault();
                event.stopPropagation();
              }}
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                actions.closeTab(pane.id, tab.id);
              }}
            >
              <X className="size-3" />
            </button>
          </TabsTrigger>
        }
      />
      <ContextMenuContent>
        <ContextMenuItem variant="destructive" onClick={() => actions.closeTab(pane.id, tab.id)}>
          Close tab
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function TabBody({ tab, onActivatePane }: { tab: TabSnapshot; onActivatePane(): void }) {
  return (
    <div
      className="grid h-full min-h-0 place-items-center overflow-hidden p-5"
      onPointerDown={onActivatePane}
    >
      <h2 className="max-w-full truncate text-3xl font-semibold tracking-tight text-muted-foreground">
        {tab.title}
      </h2>
    </div>
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
  actions: WorkspaceViewActions,
): void {
  const tabId = Number(value);

  if (!Number.isSafeInteger(tabId) || tabId === pane.activeTabId) {
    return;
  }

  if (!pane.tabs.some((tab) => tab.id === tabId)) {
    return;
  }

  actions.activateTab(pane.id, tabId);
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
