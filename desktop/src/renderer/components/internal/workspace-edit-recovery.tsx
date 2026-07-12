import { useState, useSyncExternalStore } from "react";

import {
  finalizeWorkspaceEditRecovery,
  retryWorkspaceEditRecovery,
  subscribeWorkspaceEditRecoveries,
  workspaceEditRecoveryIds,
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
  const ids = workspaceEditRecoveryIds();
  if (ids.length === 0) {
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
    } catch (caughtError) {
      setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
    } finally {
      setBusy(null);
    }
  };

  return (
    <aside className="fixed bottom-3 right-3 z-50 w-[min(32rem,calc(100vw-1.5rem))] border border-destructive/50 bg-background p-3 shadow-xl">
      <div className="text-sm font-medium">Workspace edit recovery is incomplete</div>
      <p className="mt-1 text-xs text-muted-foreground">
        Retry rollback to restore files. Finalize only if you verified the edited files and want to
        discard rollback data.
      </p>
      {ids.map((id) => (
        <div className="mt-2 flex items-center justify-between gap-3" key={id}>
          <code className="text-xs">Transaction {id}</code>
          <div className="flex gap-2">
            <button
              className="border px-2 py-1 text-xs hover:bg-muted disabled:opacity-50"
              disabled={busy !== null}
              onClick={() => void run(id, "retry")}
              type="button"
            >
              Retry rollback
            </button>
            <button
              className="border border-destructive/50 px-2 py-1 text-xs hover:bg-destructive/10 disabled:opacity-50"
              disabled={busy !== null}
              onClick={() => void run(id, "finalize")}
              type="button"
            >
              Finalize
            </button>
          </div>
        </div>
      ))}
      {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
    </aside>
  );
}
