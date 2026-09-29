import { create } from "zustand";

import type { TabId, WorkspaceId } from "@/shared/ipc";

export type QuickOpenRequest = {
  generation: number;
  workspaceId: WorkspaceId;
  tabId: TabId;
};

type QuickOpenStore = {
  request: QuickOpenRequest | null;
  requestQuickOpen(workspaceId: WorkspaceId, tabId: TabId): void;
  consumeQuickOpen(generation: number): void;
};

/** Asks an existing search tab to switch to name search and focus its input. */
export const useQuickOpenStore = create<QuickOpenStore>((set) => ({
  request: null,
  requestQuickOpen(workspaceId, tabId) {
    set((state) => ({
      request: { generation: (state.request?.generation ?? 0) + 1, workspaceId, tabId },
    }));
  },
  consumeQuickOpen(generation) {
    set((state) => (state.request?.generation === generation ? { request: null } : state));
  },
}));
