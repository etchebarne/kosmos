import type { LanguageServerWorkspaceSymbol } from "@/shared/ipc";

export type LanguageResultDocument = {
  disposed: boolean;
  generation: number;
  model: { getVersionId(): number };
};

export function isCurrentLanguageResult(
  document: LanguageResultDocument,
  generation: number,
  version: number,
  cancelled = false,
): boolean {
  return (
    !cancelled &&
    !document.disposed &&
    document.generation === generation &&
    document.model.getVersionId() === version
  );
}

export function resolvedWorkspaceSymbolIsCurrent(
  source: LanguageServerWorkspaceSymbol,
  resolved: LanguageServerWorkspaceSymbol,
): boolean {
  return (
    source.serverId === resolved.serverId &&
    source.workspaceId === resolved.workspaceId &&
    resolved.location !== null
  );
}
