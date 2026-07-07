import type {
  CommitGitChangesParams,
  CreateGitBranchParams,
  GitPathsParams,
  GitRepositorySnapshot,
  GitStash,
  GitStashParams,
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

export function initGitRepository(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "init", params);
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

export function trackGitRemoteBranch(params: SwitchGitBranchParams): Promise<boolean> {
  return requestServer(DOMAIN, "trackRemoteBranch", params);
}

export function createGitBranch(params: CreateGitBranchParams): Promise<boolean> {
  return requestServer(DOMAIN, "createBranch", params);
}

export function deleteGitBranch(params: SwitchGitBranchParams): Promise<boolean> {
  return requestServer(DOMAIN, "deleteBranch", params);
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

export function stashStagedGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "stashStaged", params);
}

export function getGitStashes(params: GitTabParams): Promise<GitStash[]> {
  return requestServer(DOMAIN, "stashes", params);
}

export function applyGitStash(params: GitStashParams): Promise<boolean> {
  return requestServer(DOMAIN, "applyStash", params);
}

export function dropGitStash(params: GitStashParams): Promise<boolean> {
  return requestServer(DOMAIN, "dropStash", params);
}

export function discardAllGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "discardAll", params);
}

export function discardStagedGitChanges(params: GitTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "discardStaged", params);
}

export type {
  CommitGitChangesParams,
  CreateGitBranchParams,
  GitPathsParams,
  GitRepositorySnapshot,
  GitStash,
  GitStashParams,
  GitTabParams,
  PullGitChangesParams,
  PushGitChangesParams,
  SwitchGitBranchParams,
} from "@/shared/ipc";
