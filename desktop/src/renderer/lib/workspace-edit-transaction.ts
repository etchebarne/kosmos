import type {
  WorkspaceEditRecovery as PersistedWorkspaceEditRecovery,
  WorkspaceEditTransactionStatus,
} from "@/shared/ipc";

export type WorkspaceEditRecoveryAction = {
  transactionId: number;
  retryRollback: boolean;
  canFinalize: boolean;
};

export type WorkspaceEditRecoveryAdapter = {
  resolve(
    recovery: PersistedWorkspaceEditRecovery,
    intent: "retryRollback" | "finalize",
  ): Promise<WorkspaceEditTransactionStatus>;
};

const recoveries = new Map<number, PersistedWorkspaceEditRecovery>();
const recoveryListeners = new Set<() => void>();
let recoveryAdapter: ((recovery: PersistedWorkspaceEditRecovery) => WorkspaceEditRecoveryAdapter) | null = null;
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

export function registerPersistedWorkspaceEditRecoveries(
  discovered: PersistedWorkspaceEditRecovery[],
  adapter: (recovery: PersistedWorkspaceEditRecovery) => WorkspaceEditRecoveryAdapter,
): void {
  recoveryAdapter = adapter;
  const next = new Map(discovered.map((recovery) => [recovery.transactionId, recovery]));
  if (sameRecoveries(recoveries, next)) return;
  recoveries.clear();
  for (const [id, recovery] of next) recoveries.set(id, recovery);
  notifyRecoveryListeners();
}

export function workspaceEditRecoveryActions(): WorkspaceEditRecoveryAction[] {
  return [...recoveries.values()].map((recovery) => ({
    transactionId: recovery.transactionId,
    retryRollback: recovery.retryRollback,
    canFinalize: recovery.canFinalize,
  }));
}

export function workspaceEditRecoveryWarnings(): Array<[number, string]> {
  return [];
}

export function dismissWorkspaceEditRecoveryWarning(_transactionId: number): void {}

export function subscribeWorkspaceEditRecoveries(listener: () => void): () => void {
  recoveryListeners.add(listener);
  return () => recoveryListeners.delete(listener);
}

export function workspaceEditRecoveryVersion(): number {
  return recoveryVersion;
}

export async function retryWorkspaceEditRecovery(transactionId: number): Promise<void> {
  await resolveRecovery(transactionId, "retryRollback");
}

export async function finalizeWorkspaceEditRecovery(transactionId: number): Promise<void> {
  await resolveRecovery(transactionId, "finalize");
}

export async function retryWorkspaceEditRecoveries(): Promise<void> {
  await Promise.allSettled(
    [...recoveries.values()]
      .filter((recovery) => recovery.retryRollback)
      .map((recovery) => retryWorkspaceEditRecovery(recovery.transactionId)),
  );
}

async function resolveRecovery(
  transactionId: number,
  intent: "retryRollback" | "finalize",
): Promise<void> {
  const recovery = recoveries.get(transactionId);
  if (!recovery || !recoveryAdapter) {
    throw new Error(`Workspace edit recovery ${transactionId} does not exist.`);
  }
  await recoveryAdapter(recovery).resolve(recovery, intent);
  recoveries.delete(transactionId);
  notifyRecoveryListeners();
}

function notifyRecoveryListeners(): void {
  recoveryVersion += 1;
  for (const listener of recoveryListeners) listener();
}

function sameRecoveries(
  current: ReadonlyMap<number, PersistedWorkspaceEditRecovery>,
  next: ReadonlyMap<number, PersistedWorkspaceEditRecovery>,
): boolean {
  if (current.size !== next.size) return false;
  for (const [id, recovery] of current) {
    const other = next.get(id);
    if (
      !other ||
      other.authorization !== recovery.authorization ||
      other.retryRollback !== recovery.retryRollback ||
      other.canFinalize !== recovery.canFinalize ||
      other.requiresAcknowledgement !== recovery.requiresAcknowledgement
    ) return false;
  }
  return true;
}
