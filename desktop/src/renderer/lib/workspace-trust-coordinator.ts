import type { WorkspaceId } from "@/shared/ipc";

import { errorMessage } from "./errors";

export type WorkspaceTrustDecision = "trust" | "cancel" | "closed";

export type WorkspaceTrustPrompt = {
  workspaceId: WorkspaceId;
  isTrusting: boolean;
  error: string | null;
};

export type WorkspaceTrustAuthorizer = () => Promise<boolean>;

export type WorkspaceTrustCoordinator = {
  request(workspaceId: WorkspaceId, authorize: WorkspaceTrustAuthorizer): Promise<WorkspaceTrustDecision>;
  trust(workspaceId: WorkspaceId): Promise<void>;
  cancel(workspaceId: WorkspaceId): void;
  close(workspaceId: WorkspaceId): void;
};

type PendingWorkspaceTrust = {
  workspaceId: WorkspaceId;
  authorize: WorkspaceTrustAuthorizer;
  decision: Promise<WorkspaceTrustDecision>;
  resolve(decision: WorkspaceTrustDecision): void;
  isTrusting: boolean;
  error: string | null;
  trustRequest: Promise<void> | null;
};

export function createWorkspaceTrustCoordinator(
  onPromptChange: (prompt: WorkspaceTrustPrompt | null) => void,
): WorkspaceTrustCoordinator {
  const pendingByWorkspace = new Map<WorkspaceId, PendingWorkspaceTrust>();
  const queue: PendingWorkspaceTrust[] = [];

  function publishPrompt(): void {
    const pending = queue[0];
    onPromptChange(
      pending
        ? {
            workspaceId: pending.workspaceId,
            isTrusting: pending.isTrusting,
            error: pending.error,
          }
        : null,
    );
  }

  function finish(pending: PendingWorkspaceTrust, decision: WorkspaceTrustDecision): void {
    pendingByWorkspace.delete(pending.workspaceId);
    const index = queue.indexOf(pending);
    if (index !== -1) {
      queue.splice(index, 1);
    }
    pending.resolve(decision);
    publishPrompt();
  }

  function pendingForActiveWorkspace(workspaceId: WorkspaceId): PendingWorkspaceTrust | undefined {
    const pending = pendingByWorkspace.get(workspaceId);
    return pending && queue[0] === pending ? pending : undefined;
  }

  return {
    request(workspaceId, authorize) {
      const existing = pendingByWorkspace.get(workspaceId);
      if (existing) {
        return existing.decision;
      }

      let resolveDecision!: (decision: WorkspaceTrustDecision) => void;
      const decision = new Promise<WorkspaceTrustDecision>((resolve) => {
        resolveDecision = resolve;
      });
      const pending: PendingWorkspaceTrust = {
        workspaceId,
        authorize,
        decision,
        resolve: resolveDecision,
        isTrusting: false,
        error: null,
        trustRequest: null,
      };
      pendingByWorkspace.set(workspaceId, pending);
      queue.push(pending);
      publishPrompt();
      return decision;
    },
    async trust(workspaceId) {
      const pending = pendingForActiveWorkspace(workspaceId);
      if (!pending) {
        return;
      }
      if (pending.trustRequest) {
        return pending.trustRequest;
      }

      pending.isTrusting = true;
      pending.error = null;
      publishPrompt();
      pending.trustRequest = (async () => {
        try {
          if (!(await pending.authorize())) {
            throw new Error("Kosmos could not trust the workspace.");
          }
          finish(pending, "trust");
        } catch (caughtError) {
          pending.isTrusting = false;
          pending.error = errorMessage(caughtError);
          pending.trustRequest = null;
          publishPrompt();
        }
      })();
      return pending.trustRequest;
    },
    cancel(workspaceId) {
      const pending = pendingForActiveWorkspace(workspaceId);
      if (pending && !pending.isTrusting) {
        finish(pending, "cancel");
      }
    },
    close(workspaceId) {
      const pending = pendingForActiveWorkspace(workspaceId);
      if (pending && !pending.isTrusting) {
        finish(pending, "closed");
      }
    },
  };
}
