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

export type EditorGitLineHunks = {
  hunks: EditorGitLineHunk[];
};

export type EditorGitLineHunk = {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
};
