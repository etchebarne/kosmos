import type {
  StagedWorkspaceEdit,
  StagedWorkspaceEditDocument,
  StagedWorkspaceEditOperation,
  WorkspaceEditTransactionStatus,
  WorkspaceEditRecovery as PersistedWorkspaceEditRecovery,
} from "@/shared/ipc";

const MAX_WORKSPACE_EDIT_RECOVERIES = 64;

export type OpenWorkspaceEditTarget = {
  document?: StagedWorkspaceEditDocument;
  validate(): void;
  apply(): void;
  undo(): void;
  complete?(): void;
  release?(): void;
};

export type WorkspaceEditTransactionAdapter = {
  validate(document: StagedWorkspaceEditDocument): void;
  preflight(document: StagedWorkspaceEditDocument): OpenWorkspaceEditTarget | null;
  preflightTargets?(edit: StagedWorkspaceEdit): OpenWorkspaceEditTarget[];
  preflightResource?(
    operation: StagedWorkspaceEditOperation,
  ): OpenWorkspaceEditTarget | null;
  commitClosed(transactionId: number): Promise<void>;
  rollbackClosed(transactionId: number): Promise<void>;
  finish(transactionId: number): Promise<void>;
  acknowledge?(transactionId: number): Promise<void>;
  finalize(transactionId: number): Promise<WorkspaceEditTransactionStatus>;
  status(transactionId: number): Promise<WorkspaceEditTransactionStatus>;
  isRecoveryRequired?(error: unknown): boolean;
  isUnknownTransaction?(error: unknown): boolean;
  reconcileUnknown?(transactionId: number): void | Promise<void>;
  reconcileCompletion?(transactionId: number): void | Promise<void>;
};

type TrackedWorkspaceEditRecovery = {
  transactionId: number;
  applied: OpenWorkspaceEditTarget[];
  targets: OpenWorkspaceEditTarget[];
  closedCommitted: boolean;
  adapter: WorkspaceEditTransactionAdapter;
  retryRollback: boolean;
  canFinalize: boolean;
  reconciledPhase?: "finishedCommitted" | "finishedRolledBack";
};

export type WorkspaceEditRecoveryAction = {
  transactionId: number;
  retryRollback: boolean;
  canFinalize: boolean;
};

const recoveries = new Map<number, TrackedWorkspaceEditRecovery>();
const recoveryWarnings = new Map<number, string>();
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
  deferCompletionAcknowledgement = false,
): Promise<void> {
  if (recoveries.size >= MAX_WORKSPACE_EDIT_RECOVERIES) {
    throw new Error(
      `Resolve an existing workspace edit recovery before applying another edit (${MAX_WORKSPACE_EDIT_RECOVERIES} retained).`,
    );
  }
  const applied: OpenWorkspaceEditTarget[] = [];
  const openTargets: OpenWorkspaceEditTarget[] = [];
  let closedCommitted = false;
  let finishing = false;
  let reconciledPhase: TrackedWorkspaceEditRecovery["reconciledPhase"];
  try {
    throwIfCancelled(signal);
    const open = openTargets;
    if (adapter.preflightTargets) {
      open.push(...adapter.preflightTargets(edit));
    } else {
      const documents: Array<OpenWorkspaceEditTarget | null | undefined> = new Array(
        edit.documents.length,
      );
      const operations = edit.operations?.length
        ? edit.operations
        : edit.documents.map((_, document) => ({ kind: "textDocument", document }) as const);
      for (const operation of operations) {
        if (operation.kind !== "textDocument") {
          const target = adapter.preflightResource?.(operation) ?? null;
          if (target) open.push(target);
          continue;
        }
        const existing = documents[operation.document];
        if (existing !== undefined) {
          continue;
        }
        const target = adapter.preflight(edit.documents[operation.document]!);
        documents[operation.document] = target;
        if (target) open.push(target);
      }
    }
    throwIfCancelled(signal);
    await adapter.commitClosed(edit.transactionId);
    closedCommitted = true;

    throwIfCancelled(signal);
    for (const target of open) {
      target.validate();
      applied.push(target);
      target.apply();
    }
    finishing = true;
    await adapter.finish(edit.transactionId);
    const completed = await adapter.status(edit.transactionId);
    if (completed.phase === "finishedCommitted") {
      for (const target of openTargets) target.complete?.();
      reconciledPhase = "finishedCommitted";
      await adapter.reconcileCompletion?.(edit.transactionId);
      if (!deferCompletionAcknowledgement) {
        await acknowledgeCompletion(adapter, edit.transactionId, completed);
      }
      releaseTargets(openTargets);
      return;
    }
    throw new Error(`Workspace edit was ${completed.phase} before model completion was acknowledged.`);
  } catch (error) {
    const status = await queryStatus(adapter, edit.transactionId);
    const recoveryFailures: unknown[] = [];
    if (status?.phase === "finishedCommitted") {
      try {
        if (reconciledPhase !== "finishedCommitted") {
          for (const target of openTargets) target.complete?.();
          reconciledPhase = "finishedCommitted";
        }
        await adapter.reconcileCompletion?.(edit.transactionId);
        if (!deferCompletionAcknowledgement) {
          await acknowledgeCompletion(adapter, edit.transactionId, status);
        }
        releaseTargets(openTargets);
        return;
      } catch (acknowledgementError) {
        recoveryFailures.push(acknowledgementError);
      }
    }
    if (status && isCommittedCleanupPhase(status.phase)) {
      try {
        if (reconciledPhase !== "finishedCommitted") {
          for (const target of openTargets) target.complete?.();
          reconciledPhase = "finishedCommitted";
        }
        await adapter.reconcileCompletion?.(edit.transactionId);
      } catch (reconciliationError) {
        recoveryFailures.push(reconciliationError);
      }
      releaseTargets(openTargets);
      recoveries.set(edit.transactionId, {
        transactionId: edit.transactionId,
        applied: [],
        targets: [],
        closedCommitted: true,
        adapter,
        retryRollback: false,
        canFinalize: true,
        reconciledPhase,
      });
      notifyRecoveryListeners();
      recoveryFailures.push(
        new Error("The workspace edit is committed, but durable cleanup still requires finalization."),
      );
      throw new WorkspaceEditRecoveryError(edit.transactionId, error, recoveryFailures);
    }
    const closedMayNeedRecovery =
      closedCommitted ||
      status?.phase === "committed" ||
      status?.phase === "recoveryRequired" ||
      adapter.isRecoveryRequired?.(error) === true;
    const outcomeAmbiguousAfterFinish = finishing && status === null;
    if (!outcomeAmbiguousAfterFinish && reconciledPhase === undefined) {
      recoveryFailures.push(
        ...(await recoverApplied(
          applied,
          closedMayNeedRecovery && status?.phase !== "finishedRolledBack",
          edit.transactionId,
          adapter,
        )),
      );
    }
    if (status === null) {
      recoveryFailures.push(
        new Error("The transaction phase could not be queried after the failed request."),
      );
    }
    if (
      recoveryFailures.length === 0 &&
      !outcomeAmbiguousAfterFinish &&
      reconciledPhase === undefined
    ) {
      try {
        await adapter.finish(edit.transactionId);
      } catch (finishError) {
        recoveryFailures.push(finishError);
      }
      if (recoveryFailures.length === 0) {
        releaseTargets(openTargets);
        throw error;
      }
    }
    recoveries.set(edit.transactionId, {
      transactionId: edit.transactionId,
      applied,
      targets: openTargets,
      closedCommitted: closedMayNeedRecovery,
      adapter,
      retryRollback: status?.retryRollback ?? true,
      canFinalize: status?.canFinalize ?? true,
      reconciledPhase,
    });
    notifyRecoveryListeners();
    throw new WorkspaceEditRecoveryError(edit.transactionId, error, recoveryFailures);
  }
}

export function workspaceEditRecoveryIds(): number[] {
  return [...recoveries.keys()];
}

export function workspaceEditRecoveryActions(): WorkspaceEditRecoveryAction[] {
  return [...recoveries.values()].map(({ transactionId, retryRollback, canFinalize }) => ({
    transactionId,
    retryRollback,
    canFinalize,
  }));
}

export function registerPersistedWorkspaceEditRecoveries(
  discovered: PersistedWorkspaceEditRecovery[],
  adapter: (recovery: PersistedWorkspaceEditRecovery) => WorkspaceEditTransactionAdapter,
): void {
  let changed = false;
  for (const recovery of discovered.slice(0, MAX_WORKSPACE_EDIT_RECOVERIES)) {
    if (
      recovery.phase !== "recoveryRequired" &&
      !isCommittedCleanupPhase(recovery.phase) &&
      !recovery.phase.startsWith("finished")
    ) continue;
    const existing = recoveries.get(recovery.transactionId);
    if (existing) {
      existing.adapter = adapter(recovery);
      existing.closedCommitted = true;
      existing.retryRollback = recovery.retryRollback;
      existing.canFinalize = recovery.canFinalize;
      continue;
    }
    recoveries.set(recovery.transactionId, {
      transactionId: recovery.transactionId,
      applied: [],
      targets: [],
      closedCommitted: true,
      adapter: adapter(recovery),
      retryRollback: recovery.retryRollback,
      canFinalize: recovery.canFinalize,
    });
    changed = true;
  }
  if (changed) notifyRecoveryListeners();
}

export function workspaceEditRecoveryWarnings(): Array<[number, string]> {
  return [...recoveryWarnings];
}

export function dismissWorkspaceEditRecoveryWarning(transactionId: number): void {
  if (recoveryWarnings.delete(transactionId)) notifyRecoveryListeners();
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
  let status: WorkspaceEditTransactionStatus;
  try {
    status = await recovery.adapter.status(transactionId);
  } catch (error) {
    if (recovery.adapter.isUnknownTransaction?.(error) !== true) throw error;
    await reconcileUnknownRecovery(recovery, error);
    return;
  }
  recovery.retryRollback = status.retryRollback;
  recovery.canFinalize = status.canFinalize;
  if (status.phase === "finishedCommitted") {
    if (recovery.reconciledPhase !== "finishedCommitted") {
      for (const target of recovery.targets) target.complete?.();
      recovery.reconciledPhase = "finishedCommitted";
    }
    await recovery.adapter.reconcileCompletion?.(transactionId);
    await acknowledgeFinishedRecovery(recovery, status);
    releaseTargets(recovery.targets);
    recoveries.delete(transactionId);
    notifyRecoveryListeners();
    return;
  }
  if (status.phase === "finishedRolledBack" || status.phase === "finishedUncommitted") {
    if (recovery.reconciledPhase !== "finishedRolledBack") {
      const failures = await recoverApplied(
        recovery.applied,
        false,
        recovery.transactionId,
        recovery.adapter,
      );
      if (failures.length > 0) {
        throw new WorkspaceEditRecoveryError(transactionId, "rollback reconciliation", failures);
      }
      recovery.reconciledPhase = "finishedRolledBack";
    }
    await acknowledgeFinishedRecovery(recovery, status);
    releaseTargets(recovery.targets);
    recoveries.delete(transactionId);
    notifyRecoveryListeners();
    return;
  }
  if (isCommittedCleanupPhase(status.phase)) {
    if (recovery.reconciledPhase !== "finishedCommitted") {
      for (const target of recovery.targets) target.complete?.();
      recovery.reconciledPhase = "finishedCommitted";
    }
    await recovery.adapter.reconcileCompletion?.(transactionId);
    releaseTargets(recovery.targets);
    const finalized = await recovery.adapter.finalize(transactionId);
    recovery.retryRollback = finalized.retryRollback;
    recovery.canFinalize = finalized.canFinalize;
    return retryWorkspaceEditRecovery(transactionId);
  }
  const closedCommitted =
    recovery.closedCommitted ||
    status.phase === "committed" ||
    status.phase === "recoveryRequired";
  const failures = await recoverApplied(
    recovery.applied,
    closedCommitted,
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
  releaseTargets(recovery.targets);
  recoveries.delete(transactionId);
  notifyRecoveryListeners();
}

export async function finalizeWorkspaceEditRecovery(transactionId: number): Promise<void> {
  const recovery = recoveries.get(transactionId);
  if (!recovery) {
    throw new Error(`Workspace edit recovery ${transactionId} does not exist.`);
  }
  const status = await recovery.adapter.finalize(transactionId);
  if (status.phase === "finishedCommitted") {
    if (recovery.reconciledPhase !== "finishedCommitted") {
      for (const target of recovery.targets) target.complete?.();
      recovery.reconciledPhase = "finishedCommitted";
    }
    await recovery.adapter.reconcileCompletion?.(transactionId);
  } else if (status.phase === "finishedRolledBack") {
    const failures = await recoverApplied(
      recovery.applied,
      false,
      transactionId,
      recovery.adapter,
    );
    if (failures.length > 0) {
      throw new WorkspaceEditRecoveryError(transactionId, "finalization rollback", failures);
    }
    recovery.reconciledPhase = "finishedRolledBack";
  } else {
    throw new Error(`Workspace edit finalization returned non-terminal phase ${status.phase}.`);
  }
  await acknowledgeFinishedRecovery(recovery, status);
  releaseTargets(recovery.targets);
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

function isCommittedCleanupPhase(
  phase: WorkspaceEditTransactionStatus["phase"],
): boolean {
  return phase === "finishingCommitted" || phase === "committedCleanupRequired";
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

async function acknowledgeFinishedRecovery(
  recovery: TrackedWorkspaceEditRecovery,
  status: WorkspaceEditTransactionStatus,
): Promise<void> {
  if (!status.requiresAcknowledgement) return;
  try {
    await acknowledgeCompletion(recovery.adapter, recovery.transactionId, status);
  } catch (error) {
    if (recovery.adapter.isUnknownTransaction?.(error) !== true) throw error;
    recoveryWarnings.set(
      recovery.transactionId,
      `Workspace edit ${recovery.transactionId} was reconciled, but its server outcome expired before acknowledgement. The workspace was fully refreshed and editor locks were released.`,
    );
    await recovery.adapter.reconcileUnknown?.(recovery.transactionId);
  }
}

async function reconcileUnknownRecovery(
  recovery: TrackedWorkspaceEditRecovery,
  error: unknown,
): Promise<void> {
  const failures: unknown[] = [];
  try {
    await recovery.adapter.reconcileUnknown?.(recovery.transactionId);
  } catch (reconcileError) {
    failures.push(reconcileError);
  }
  releaseTargets(recovery.targets);
  recoveries.delete(recovery.transactionId);
  const details = failures.length > 0
    ? ` Some local targets could not be reset safely: ${failures.map(errorMessage).join("; ")}.`
    : "";
  recoveryWarnings.set(
    recovery.transactionId,
    `Workspace edit ${recovery.transactionId} is no longer known by the server (${errorMessage(error)}). A full workspace reconciliation was requested and editor locks were released.${details}`,
  );
  notifyRecoveryListeners();
}

async function acknowledgeCompletion(
  adapter: WorkspaceEditTransactionAdapter,
  transactionId: number,
  status: WorkspaceEditTransactionStatus,
): Promise<void> {
  if (!status.requiresAcknowledgement) return;
  if (!adapter.acknowledge) {
    throw new Error("Workspace edit completion acknowledgement is unavailable.");
  }
  await adapter.acknowledge(transactionId);
}

function notifyRecoveryListeners(): void {
  recoveryVersion += 1;
  for (const listener of recoveryListeners) {
    listener();
  }
}

function releaseTargets(targets: OpenWorkspaceEditTarget[]): void {
  for (const target of targets) {
    target.release?.();
  }
}

export async function retryWorkspaceEditRecoveries(): Promise<void> {
  await Promise.allSettled(workspaceEditRecoveryIds().map(retryWorkspaceEditRecovery));
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
