import type { TabId, WorkspaceId } from "./ids";
import type { GitChangeKind } from "./git";

export type GetFileTreeParams = {
  workspaceId?: WorkspaceId | null;
  tabId?: TabId | null;
};

export type GetFileTreeChildrenParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  path: string;
};

export type GetFileTreeGitStatusParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
};

export type SetFileTreeExpandedPathsParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  expandedPaths: string[];
};

export type FileTreeEntryKind = "directory" | "file";

export type CreateFileTreeEntryParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  parentPath?: string | null;
  name: string;
  kind: FileTreeEntryKind;
};

export type RenameFileTreeEntryParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  sourcePath: string;
  destinationPath: string;
};

export type TransferFileTreeEntriesParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  sourcePaths: string[];
  targetDirectoryPath?: string | null;
};

export type DeleteFileTreeEntriesParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  paths: string[];
};

export type ResolveFileTreePathParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  path?: string | null;
};

export type FileTreeResolvedPath = {
  path: string;
};

export type FileTreeSnapshot = {
  root: string;
  rootPath: string;
  paths: string[];
  expandedPaths: string[];
  deferredPaths: string[];
};

export type FileTreeChildrenSnapshot = {
  paths: string[];
  deferredPaths: string[];
};

export type FileTreeGitStatusSnapshot = {
  entries: FileTreeGitStatusEntry[];
};

export type FileTreeGitStatusEntry = {
  path: string;
  status: GitChangeKind;
};
