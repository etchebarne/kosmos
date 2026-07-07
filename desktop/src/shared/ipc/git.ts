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

export type GitTabParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
};

export type GitPathsParams = GitTabParams & {
  paths: string[];
};

export type CommitGitChangesParams = GitTabParams & {
  message: string;
};

export type SwitchGitBranchParams = GitTabParams & {
  branch: string;
};

export type PullGitChangesParams = GitTabParams & {
  rebase: boolean;
};

export type PushGitChangesParams = GitTabParams & {
  force: boolean;
};
