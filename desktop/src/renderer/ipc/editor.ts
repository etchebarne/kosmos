import type {
  EditorDocument,
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

export function getEditorGitLineHunks(params: EditorTabParams): Promise<EditorGitLineHunks> {
  return requestServer(DOMAIN, "gitLineHunks", params);
}

export function saveEditorDocument(
  params: SaveEditorDocumentParams,
  cancellation?: RequestCancellation,
): Promise<EditorSave> {
  return requestServer<EditorSave>(DOMAIN, "save", params, cancellation);
}

export function openEditorSession(params: OpenEditorSessionParams): Promise<EditorDocument> {
  return requestServer(DOMAIN, "openSession", params);
}

export function restoreEditorSession(params: RestoreEditorSessionParams): Promise<EditorDocument> {
  return requestServer(DOMAIN, "restoreSession", params);
}

export function changeEditorSession(params: ChangeEditorSessionParams): Promise<EditorDocument> {
  return requestServer(DOMAIN, "changeSession", params);
}
