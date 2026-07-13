import type { GitStatus } from "@pierre/trees";

import type { GitChangeKind } from "@/shared/ipc";

export function pierreGitStatus(kind: GitChangeKind): GitStatus {
  switch (kind) {
    case "added":
      return "added";
    case "deleted":
      return "deleted";
    case "ignored":
      return "ignored";
    case "renamed":
      return "renamed";
    case "untracked":
      return "untracked";
    case "conflicted":
    case "modified":
      return "modified";
  }
}
