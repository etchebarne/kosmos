import type { TabId, WorkspaceId } from "./ids";

export type GetFileTreeParams = {
  workspaceId?: WorkspaceId | null;
  tabId?: TabId | null;
};

export type SetFileTreeExpandedPathsParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  expandedPaths: string[];
};

export type FileTreeSnapshot = {
  root: string;
  paths: string[];
  expandedPaths: string[];
};
