import type { LanguageServerSnapshot } from "@/shared/ipc";

export function languageServerOperationInProgress(server: LanguageServerSnapshot): boolean {
  return server.installationState === "installing"
    || server.installationState === "uninstalling"
    || server.runtimeState === "restarting";
}

export function statusRetryDelay(attempt: number): number {
  return Math.min(250 * 2 ** Math.max(0, attempt), 4_000);
}

export function pendingServersAfterStatus(
  pending: Record<string, true>,
  server: LanguageServerSnapshot,
): Record<string, true> {
  if (languageServerOperationInProgress(server)) {
    return pending[server.id] ? pending : { ...pending, [server.id]: true };
  }
  if (!pending[server.id]) {
    return pending;
  }
  const next = { ...pending };
  delete next[server.id];
  return next;
}
