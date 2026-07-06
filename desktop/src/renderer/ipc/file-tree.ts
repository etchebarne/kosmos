import type {
  FileTreeSnapshot,
  GetFileTreeParams,
  SetFileTreeExpandedPathsParams,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "fileTree";

export function getFileTree(params: GetFileTreeParams = {}): Promise<FileTreeSnapshot> {
  return requestServer(DOMAIN, "get", params);
}

export function setFileTreeExpandedPaths(
  params: SetFileTreeExpandedPathsParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "setExpandedPaths", params);
}

export type {
  FileTreeSnapshot,
  GetFileTreeParams,
  SetFileTreeExpandedPathsParams,
} from "@/shared/ipc";
