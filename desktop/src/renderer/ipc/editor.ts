import type {
  EditorDocument,
  EditorDocumentSync,
  EditorSessionAck,
  EditorSave,
  EditorGitLineHunks,
  EditorTabParams,
  ChangeEditorSessionParams,
  OpenEditorLocationParams,
  OpenEditorLocationPayload,
  OpenEditorSessionParams,
  RestoreEditorSessionParams,
  OpenEditorTabParams,
  SaveEditorDocumentParams,
  SyncEditorDocumentParams,
  WorkspaceListSnapshot,
} from "@/shared/ipc";

import { requestServer } from "./transport";
import type { RequestCancellation } from "./transport";

const DOMAIN = "editor";

export function openEditorTab(params: OpenEditorTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "openTab", params);
}

export function openEditorLocation(
  params: OpenEditorLocationParams,
): Promise<OpenEditorLocationPayload> {
  return requestServer(DOMAIN, "openLocation", params);
}

export function getEditorDocument(params: EditorTabParams): Promise<EditorDocument> {
  return requestServer(DOMAIN, "document", params);
}

/** Fetches the document only if it changed since the caller's known version. */
export function syncEditorDocument(params: SyncEditorDocumentParams): Promise<EditorDocumentSync> {
  return requestServer(DOMAIN, "sync", params);
}

export function getEditorGitLineHunks(params: EditorTabParams): Promise<EditorGitLineHunks> {
  return requestServer(DOMAIN, "gitLineHunks", params);
}

/** Git line markers for the tab's unsaved text, or null when they are not available. */
export function getUnsavedEditorGitLineHunks(
  params: EditorTabParams,
): Promise<EditorGitLineHunks | null> {
  return requestServer(DOMAIN, "unsavedGitLineHunks", params);
}

export function saveEditorDocument(
  params: SaveEditorDocumentParams,
  cancellation?: RequestCancellation,
): Promise<EditorSave> {
  return requestServer<EditorSave>(DOMAIN, "save", params, cancellation);
}

export function openEditorSession(params: OpenEditorSessionParams): Promise<EditorSessionAck> {
  return requestServer(DOMAIN, "openSession", params);
}

export function restoreEditorSession(
  params: RestoreEditorSessionParams,
): Promise<EditorSessionAck> {
  return requestServer(DOMAIN, "restoreSession", params);
}

/** Discards the tab's server session, including unsaved edits. */
export function closeEditorSession(params: EditorTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "closeSession", params);
}

export function changeEditorSession(params: ChangeEditorSessionParams): Promise<EditorSessionAck> {
  return requestServer(DOMAIN, "changeSession", params);
}
