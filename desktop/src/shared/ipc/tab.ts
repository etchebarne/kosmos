import type { PaneId, TabId, WorkspaceId } from "./ids";
import type { SplitAxis } from "./pane";

export type TabKind = "blank" | "diff" | "fileTree" | "editor" | "git" | "search" | "terminal";
export type OpenableTabKind = Exclude<TabKind, "diff" | "editor">;

export type TabLifecycle = "ephemeral" | "keepAlive";

export type OpenTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId?: PaneId | null;
  title?: string;
  kind?: OpenableTabKind;
};

export type ActivateTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  tabId: TabId;
};

export type SetTabKindParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  tabId: TabId;
  kind: OpenableTabKind;
};

export type CloseTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  tabId: TabId;
};

export type MoveTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  targetPaneId: PaneId;
  tabId: TabId;
  targetIndex: number;
};

export type SplitTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  targetPaneId?: PaneId | null;
  tabId: TabId;
  axis: SplitAxis;
  newPaneFirst?: boolean;
};

export type TabSnapshot = {
  id: TabId;
  title: string;
  kind: TabKind;
  lifecycle: TabLifecycle;
};
