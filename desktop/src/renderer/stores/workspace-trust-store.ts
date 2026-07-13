import { create } from "zustand";

import {
  createWorkspaceTrustCoordinator,
  type WorkspaceTrustAuthorizer,
  type WorkspaceTrustDecision,
  type WorkspaceTrustPrompt,
} from "@/renderer/lib/workspace-trust-coordinator";
import type { WorkspaceId } from "@/shared/ipc";

type WorkspaceTrustStore = {
  prompt: WorkspaceTrustPrompt | null;
  requestWorkspaceTrust(
    workspaceId: WorkspaceId,
    authorize: WorkspaceTrustAuthorizer,
  ): Promise<WorkspaceTrustDecision>;
  trustWorkspace(workspaceId: WorkspaceId): Promise<void>;
  cancelWorkspaceTrust(workspaceId: WorkspaceId): void;
  closeWorkspaceTrust(workspaceId: WorkspaceId): void;
};

export const useWorkspaceTrustStore = create<WorkspaceTrustStore>((set) => {
  const coordinator = createWorkspaceTrustCoordinator((prompt) => set({ prompt }));

  return {
    prompt: null,
    requestWorkspaceTrust: coordinator.request,
    trustWorkspace: coordinator.trust,
    cancelWorkspaceTrust: coordinator.cancel,
    closeWorkspaceTrust: coordinator.close,
  };
});

export function requestWorkspaceTrust(
  workspaceId: WorkspaceId,
  authorize: WorkspaceTrustAuthorizer,
): Promise<WorkspaceTrustDecision> {
  return useWorkspaceTrustStore.getState().requestWorkspaceTrust(workspaceId, authorize);
}
