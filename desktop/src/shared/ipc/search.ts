import type { EditorDocument } from "./editor";
import type { TabId, WorkspaceId } from "./ids";

export type SearchMode = "name" | "content";

export type SearchWorkspaceParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  query: string;
  mode: SearchMode;
};

export type SearchDocumentParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  path: string;
};

export type SearchMatch = {
  path: string;
  lineNumber: number | null;
  preview: string | null;
};

export type WorkspaceSearchResults = {
  matches: SearchMatch[];
  limitReached: boolean;
};

export type SearchDocument = EditorDocument;
