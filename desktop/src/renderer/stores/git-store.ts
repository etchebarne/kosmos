import { create } from "zustand";

import type { WorkspaceId } from "@/shared/ipc";

type GitStore = {
  revisions: Partial<Record<WorkspaceId, number>>;
  bumpGitRevision(workspaceId: WorkspaceId): void;
};

export const useGitStore = create<GitStore>((set) => ({
  revisions: {},
  bumpGitRevision(workspaceId) {
    set((state) => ({
      revisions: {
        ...state.revisions,
        [workspaceId]: (state.revisions[workspaceId] ?? 0) + 1,
      },
    }));
  },
}));
