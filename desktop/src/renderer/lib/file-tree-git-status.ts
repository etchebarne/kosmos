import type { GitStatus } from "@pierre/trees";

import type { FileTreeGitStatusEntry } from "@/shared/ipc";

import { pierreGitStatus } from "./git-status";

export function fileTreeGitStatus(entry: FileTreeGitStatusEntry): GitStatus {
  return pierreGitStatus(entry.unstaged ?? entry.staged ?? "modified");
}
