import { useState, useSyncExternalStore } from "react";

import {
  dismissWorkspaceEditRecoveryWarning,
  finalizeWorkspaceEditRecovery,
  retryWorkspaceEditRecovery,
  subscribeWorkspaceEditRecoveries,
  workspaceEditRecoveryActions,
  workspaceEditRecoveryWarnings,
  workspaceEditRecoveryVersion,
} from "@/renderer/lib/workspace-edit-transaction";

export function WorkspaceEditRecovery() {
  useSyncExternalStore(
    subscribeWorkspaceEditRecoveries,
    workspaceEditRecoveryVersion,
    workspaceEditRecoveryVersion,
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const recoveries = workspaceEditRecoveryActions();
  const warnings = workspaceEditRecoveryWarnings();
  if (recoveries.length === 0 && warnings.length === 0) {
    return null;
  }

  const run = async (id: number, action: "retry" | "finalize") => {
    setBusy(id);
    setError(null);
    try {
      if (action === "retry") {
        await retryWorkspaceEditRecovery(id);
      } else {
        await finalizeWorkspaceEditRecovery(id);
      }
      window.dispatchEvent(new CustomEvent("kosmos:workspace-edit-applied"));
    } catch (caughtError) {
      setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
    } finally {
      setBusy(null);
    }
  };

  return (
    <aside className="fixed bottom-3 right-3 z-50 w-[min(32rem,calc(100vw-1.5rem))] border border-destructive/50 bg-background p-3 shadow-xl">
      <div className="text-sm font-medium">
        {recoveries.length > 0 ? "Workspace edit recovery is incomplete" : "Workspace edit reconciled"}
      </div>
      {recoveries.length > 0 ? (
        <p className="mt-1 text-xs text-muted-foreground">
          Committed edits only offer cleanup finalization. Uncommitted edits can retry rollback or
          be explicitly finalized after you verify the affected files.
        </p>
      ) : null}
      {recoveries.map((recovery) => (
        <div className="mt-2 flex items-center justify-between gap-3" key={recovery.transactionId}>
          <code className="text-xs">Transaction {recovery.transactionId}</code>
          <div className="flex gap-2">
            {recovery.retryRollback ? (
              <button
                className="border px-2 py-1 text-xs hover:bg-muted disabled:opacity-50"
                disabled={busy !== null}
                onClick={() => void run(recovery.transactionId, "retry")}
                type="button"
              >
                Retry rollback
              </button>
            ) : null}
            {recovery.canFinalize ? (
              <button
                className="border border-destructive/50 px-2 py-1 text-xs hover:bg-destructive/10 disabled:opacity-50"
                disabled={busy !== null}
                onClick={() => void run(recovery.transactionId, "finalize")}
                type="button"
              >
                {recovery.retryRollback ? "Finalize" : "Retry finalize"}
              </button>
            ) : null}
          </div>
        </div>
      ))}
      {warnings.map(([id, warning]) => (
        <div className="mt-2 flex items-start justify-between gap-3" key={`warning-${id}`}>
          <p className="text-xs text-amber-700 dark:text-amber-300">{warning}</p>
          <button
            className="border px-2 py-1 text-xs hover:bg-muted"
            onClick={() => dismissWorkspaceEditRecoveryWarning(id)}
            type="button"
          >
            Dismiss
          </button>
        </div>
      ))}
      {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
    </aside>
  );
}
