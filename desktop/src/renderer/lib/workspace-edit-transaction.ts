import type {
  StagedWorkspaceEdit,
  StagedWorkspaceEditDocument,
  WorkspaceEditTransactionStatus,
} from "@/shared/ipc";

const MAX_WORKSPACE_EDIT_RECOVERIES = 16;

export type OpenWorkspaceEditTarget = {
  document: StagedWorkspaceEditDocument;
  validate(): void;
  apply(): void;
  undo(): void;
};

export type WorkspaceEditTransactionAdapter = {
  validate(document: StagedWorkspaceEditDocument): void;
  preflight(document: StagedWorkspaceEditDocument): OpenWorkspaceEditTarget | null;
  commitClosed(transactionId: number): Promise<void>;
  rollbackClosed(transactionId: number): Promise<void>;
  finish(transactionId: number): Promise<void>;
  finalize(transactionId: number): Promise<void>;
  status(transactionId: number): Promise<WorkspaceEditTransactionStatus>;
  isRecoveryRequired?(error: unknown): boolean;
};

type WorkspaceEditRecovery = {
  transactionId: number;
  applied: OpenWorkspaceEditTarget[];
  closedCommitted: boolean;
  adapter: WorkspaceEditTransactionAdapter;
};

const recoveries = new Map<number, WorkspaceEditRecovery>();
const recoveryListeners = new Set<() => void>();
let recoveryVersion = 0;

export function replaceWorkspaceEditModel(
  model: { getValue(): string; setValue(value: string): void },
  value: string,
): void {
  model.setValue(value);
  if (model.getValue() !== value) {
    throw new Error("Model rejected the workspace edit replacement.");
  }
}

export class WorkspaceEditRecoveryError extends Error {
  constructor(
    readonly transactionId: number,
    cause: unknown,
    recoveryFailures: unknown[],
  ) {
    const original = errorMessage(cause);
    const recovery = recoveryFailures.map(errorMessage).join("; ");
    super(
      `Workspace edit ${transactionId} failed: ${original}. Recovery is incomplete: ${recovery}. ` +
        `Retry recovery or explicitly finalize transaction ${transactionId}.`,
    );
    this.name = "WorkspaceEditRecoveryError";
  }
}

export async function applyWorkspaceEditTransaction(
  edit: StagedWorkspaceEdit,
  adapter: WorkspaceEditTransactionAdapter,
  signal?: AbortSignal,
): Promise<void> {
  if (recoveries.size >= MAX_WORKSPACE_EDIT_RECOVERIES) {
    throw new Error(
      `Resolve an existing workspace edit recovery before applying another edit (${MAX_WORKSPACE_EDIT_RECOVERIES} retained).`,
    );
  }
  const applied: OpenWorkspaceEditTarget[] = [];
  let closedCommitted = false;
  try {
    throwIfCancelled(signal);
    const open = edit.documents
      .map((document) => adapter.preflight(document))
      .filter((target): target is OpenWorkspaceEditTarget => target !== null);
    throwIfCancelled(signal);
    await adapter.commitClosed(edit.transactionId);
    closedCommitted = true;

    // Commit is an asynchronous boundary, so all models are checked together before any model changes.
    throwIfCancelled(signal);
    for (const document of edit.documents) {
      adapter.validate(document);
    }
    throwIfCancelled(signal);
    for (const target of open) {
      target.validate();
      applied.push(target);
      target.apply();
    }
    await adapter.finish(edit.transactionId);
    const completed = await adapter.status(edit.transactionId);
    if (completed.phase === "finishedCommitted") {
      return;
    }
    throw new Error(`Workspace edit was ${completed.phase} before model completion was acknowledged.`);
  } catch (error) {
    const status = await queryStatus(adapter, edit.transactionId);
    if (status?.phase === "finishedCommitted") {
      return;
    }
    const closedMayNeedRecovery =
      closedCommitted ||
      status?.phase === "committed" ||
      status?.phase === "recoveryRequired" ||
      adapter.isRecoveryRequired?.(error) === true;
    const recoveryFailures = await recoverApplied(
      applied,
      closedMayNeedRecovery && status?.phase !== "finishedRolledBack",
      edit.transactionId,
      adapter,
    );
    if (status === null) {
      recoveryFailures.push(
        new Error("The transaction phase could not be queried after the failed request."),
      );
    }
    if (recoveryFailures.length === 0) {
      try {
        await adapter.finish(edit.transactionId);
      } catch (finishError) {
        recoveryFailures.push(finishError);
      }
      if (recoveryFailures.length === 0) {
        throw error;
      }
    }
    recoveries.set(edit.transactionId, {
      transactionId: edit.transactionId,
      applied,
      closedCommitted: closedMayNeedRecovery,
      adapter,
    });
    notifyRecoveryListeners();
    throw new WorkspaceEditRecoveryError(edit.transactionId, error, recoveryFailures);
  }
}

export function workspaceEditRecoveryIds(): number[] {
  return [...recoveries.keys()];
}

export function subscribeWorkspaceEditRecoveries(listener: () => void): () => void {
  recoveryListeners.add(listener);
  return () => recoveryListeners.delete(listener);
}

export function workspaceEditRecoveryVersion(): number {
  return recoveryVersion;
}

export async function retryWorkspaceEditRecovery(transactionId: number): Promise<void> {
  const recovery = recoveries.get(transactionId);
  if (!recovery) {
    throw new Error(`Workspace edit recovery ${transactionId} does not exist.`);
  }
  const status = await recovery.adapter.status(transactionId);
  if (status.phase === "finishedCommitted") {
    recoveries.delete(transactionId);
    notifyRecoveryListeners();
    return;
  }
  const closedCommitted =
    recovery.closedCommitted ||
    status.phase === "committed" ||
    status.phase === "recoveryRequired";
  const failures = await recoverApplied(
    recovery.applied,
    closedCommitted && status.phase !== "finishedRolledBack",
    recovery.transactionId,
    recovery.adapter,
  );
  if (failures.length > 0) {
    throw new WorkspaceEditRecoveryError(
      transactionId,
      "recovery retry failed",
      failures,
    );
  }
  if (!status.phase.startsWith("finished")) {
    await recovery.adapter.finish(transactionId);
  }
  recoveries.delete(transactionId);
  notifyRecoveryListeners();
}

export async function finalizeWorkspaceEditRecovery(transactionId: number): Promise<void> {
  const recovery = recoveries.get(transactionId);
  if (!recovery) {
    throw new Error(`Workspace edit recovery ${transactionId} does not exist.`);
  }
  await recovery.adapter.finalize(transactionId);
  recoveries.delete(transactionId);
  notifyRecoveryListeners();
}

async function recoverApplied(
  applied: OpenWorkspaceEditTarget[],
  closedCommitted: boolean,
  transactionId: number,
  adapter: WorkspaceEditTransactionAdapter,
): Promise<unknown[]> {
  const failures: unknown[] = [];
  for (const target of [...applied].reverse()) {
    try {
      target.undo();
      applied.splice(applied.indexOf(target), 1);
    } catch (error) {
      failures.push(error);
    }
  }
  if (closedCommitted) {
    try {
      await adapter.rollbackClosed(transactionId);
    } catch (error) {
      failures.push(error);
    }
  }
  return failures;
}

async function queryStatus(
  adapter: WorkspaceEditTransactionAdapter,
  transactionId: number,
): Promise<WorkspaceEditTransactionStatus | null> {
  try {
    return await adapter.status(transactionId);
  } catch {
    return null;
  }
}

function notifyRecoveryListeners(): void {
  recoveryVersion += 1;
  for (const listener of recoveryListeners) {
    listener();
  }
}

function throwIfCancelled(signal?: AbortSignal): void {
  if (signal?.aborted) {
    throw signal.reason instanceof Error
      ? signal.reason
      : new Error("Workspace edit application was cancelled.");
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
