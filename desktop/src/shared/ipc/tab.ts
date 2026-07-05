import type { PaneId, TabId, WorkspaceId } from "./ids";
import type { SplitAxis } from "./pane";

export type TabKind = "blank" | "fileTree" | "editor" | "git" | "search" | "terminal" | "settings";

export type OpenTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId?: PaneId | null;
  title?: string;
  kind?: TabKind;
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
  kind: TabKind;
};

export type CloseTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
  tabId: TabId;
};

export type ReorderTabParams = {
  workspaceId?: WorkspaceId | null;
  paneId: PaneId;
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
};
