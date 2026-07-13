import type {
  LanguageServerDiagnosticsChanged,
  LanguageServerDiagnosticSnapshot,
} from "@/shared/ipc";

export type CurrentLanguageDocument = {
  disposed: boolean;
  opened: boolean;
  generation: number;
  syncedVersion: number;
  model: { getVersionId(): number };
};

export function isCurrentDiagnostics(
  event: Pick<LanguageServerDiagnosticsChanged, "generation" | "version">,
  document: CurrentLanguageDocument,
): boolean {
  return (
    !document.disposed &&
    document.opened &&
    document.generation === event.generation &&
    document.syncedVersion === event.version &&
    document.model.getVersionId() === event.version
  );
}

export function diagnosticOwners(
  snapshots: readonly Pick<LanguageServerDiagnosticSnapshot, "serverId">[],
): string[] {
  return snapshots.map((snapshot) => snapshot.serverId);
}
