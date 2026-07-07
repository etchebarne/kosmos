import type {
  CommitGitChangesParams,
  GitPathsParams,
  GitRepositorySnapshot,
  GitTabParams,
  PullGitChangesParams,
  PushGitChangesParams,
  SwitchGitBranchParams,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "git";

export function getGitStatus(params: GitTabParams): Promise<GitRepositorySnapshot> {
  return requestServer(DOMAIN, "status", params);
}

export function stageGitPaths(params: GitPathsParams): Promise<boolean> {
  return requestServer(DOMAIN, "stagePaths", params);
}

export function unstageGitPaths(params: GitPathsParams): Promise<boolean> {
  return requestServer(DOMAIN, "unstagePaths", params);
}

export function stageAllGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "stageAll", params);
}

export function unstageAllGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "unstageAll", params);
}

export function commitGitChanges(params: CommitGitChangesParams): Promise<boolean> {
  return requestServer(DOMAIN, "commit", params);
}

export function switchGitBranch(params: SwitchGitBranchParams): Promise<boolean> {
  return requestServer(DOMAIN, "switchBranch", params);
}

export function fetchGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "fetch", params);
}

export function pullGitChanges(params: PullGitChangesParams): Promise<boolean> {
  return requestServer(DOMAIN, "pull", params);
}

export function pushGitChanges(params: PushGitChangesParams): Promise<boolean> {
  return requestServer(DOMAIN, "push", params);
}

export function stashGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "stash", params);
}

export function discardAllGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "discardAll", params);
}

export function discardStagedGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "discardStaged", params);
}

export type {
  CommitGitChangesParams,
  GitPathsParams,
  GitRepositorySnapshot,
  GitTabParams,
  PullGitChangesParams,
  PushGitChangesParams,
  SwitchGitBranchParams,
} from "@/shared/ipc";
