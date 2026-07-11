import type {
  EditorDocument,
  EditorGitLineHunks,
  EditorTabParams,
  OpenEditorTabParams,
  SaveEditorDocumentParams,
  WorkspaceListSnapshot,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "editor";

export function openEditorTab(params: OpenEditorTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "openTab", params);
}

export function getEditorDocument(params: EditorTabParams): Promise<EditorDocument> {
  return requestServer(DOMAIN, "document", params);
}

export function getEditorGitLineHunks(params: EditorTabParams): Promise<EditorGitLineHunks> {
  return requestServer(DOMAIN, "gitLineHunks", params);
}

export function saveEditorDocument(params: SaveEditorDocumentParams): Promise<boolean> {
  return requestServer(DOMAIN, "save", params);
}
