import type { PaneId, WorkspaceId } from "./ids";
import type { PaneNodeSnapshot } from "./pane";

export type OpenWorkspaceParams = {
  path: string;
};

export type ActivateWorkspaceParams = {
  workspaceId: WorkspaceId;
};

export type CloseWorkspaceParams = {
  workspaceId?: WorkspaceId | null;
};

export type WorkspaceListSnapshot = {
  activeWorkspaceId: WorkspaceId | null;
  workspaces: WorkspaceSnapshot[];
};

export type WorkspaceSnapshot = {
  id: WorkspaceId;
  name: string;
  directory: string;
  activePaneId: PaneId;
  root: PaneNodeSnapshot;
};
