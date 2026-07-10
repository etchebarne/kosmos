import type { TabId, WorkspaceId } from "./ids";

export type GitChangeKind =
  | "added"
  | "conflicted"
  | "deleted"
  | "ignored"
  | "modified"
  | "renamed"
  | "untracked";

export type GitBranch = {
  name: string;
  current: boolean;
  remote: boolean;
  upstream?: string | null;
};

export type GitChange = {
  path: string;
  originalPath?: string | null;
  staged?: GitChangeKind | null;
  unstaged?: GitChangeKind | null;
  isStaged: boolean;
  isUnstaged: boolean;
};

export type GitRepositorySnapshot = {
  repositoryRoot: string;
  branch?: string | null;
  upstream?: string | null;
  latestCommit?: string | null;
  ahead: number;
  behind: number;
  insertions: number;
  deletions: number;
  branches: GitBranch[];
  changes: GitChange[];
};

export type GitStash = {
  selector: string;
  commit: string;
  timestamp: number;
  message: string;
};

export type GitRemote = {
  name: string;
  fetchUrls: string[];
  pushUrls: string[];
};

export type GitTag = {
  name: string;
  target: string;
};

export type GitDiffSectionKind = "staged" | "unstaged";

export type GitDiffSection = {
  kind: GitDiffSectionKind;
  patch: string;
};

export type GitDiffFile = {
  path: string;
  originalPath?: string | null;
  staged?: GitChangeKind | null;
  unstaged?: GitChangeKind | null;
  sections: GitDiffSection[];
};

export type GitDiff = {
  focusedPath?: string | null;
  files: GitDiffFile[];
};

export type GitTabParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
};

export type GitPathsParams = GitTabParams & {
  paths: string[];
};

export type OpenGitDiffTabParams = GitTabParams & {
  path: string;
};

export type CommitGitChangesParams = GitTabParams & {
  message: string;
};

export type SwitchGitBranchParams = GitTabParams & {
  branch: string;
};

export type CreateGitBranchParams = GitTabParams & {
  name: string;
  startPoint: string;
};

export type PullGitChangesParams = GitTabParams & {
  rebase: boolean;
};

export type PushGitChangesParams = GitTabParams & {
  force: boolean;
};

export type GitStashParams = GitTabParams & {
  selector: string;
};

export type GitRemoteParams = GitTabParams & {
  name: string;
};

export type AddGitRemoteParams = GitRemoteParams & {
  url: string;
};

export type GitTagParams = GitTabParams & {
  name: string;
};
