import type { TabId, WorkspaceId } from "./ids";

export type EditorTabParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
};

export type OpenEditorTabParams = EditorTabParams & {
  path: string;
};

export type SaveEditorDocumentParams = EditorTabParams & {
  content: string;
};

export type EditorDocument = {
  path: string;
  content: string;
};
