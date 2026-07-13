import type {
  SearchDocument,
  SearchDocumentParams,
  SearchWorkspaceParams,
  WorkspaceSearchResults,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "search";

export function searchWorkspace(params: SearchWorkspaceParams): Promise<WorkspaceSearchResults> {
  return requestServer(DOMAIN, "query", params);
}

export function getSearchDocument(params: SearchDocumentParams): Promise<SearchDocument> {
  return requestServer(DOMAIN, "document", params);
}
