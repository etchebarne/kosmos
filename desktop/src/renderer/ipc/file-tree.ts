import type {
  CreateFileTreeEntryParams,
  DeleteFileTreeEntriesParams,
  FileTreeChildrenSnapshot,
  FileTreeResolvedPath,
  FileTreeSnapshot,
  GetFileTreeChildrenParams,
  GetFileTreeParams,
  RenameFileTreeEntryParams,
  ResolveFileTreePathParams,
  SetFileTreeExpandedPathsParams,
  TransferFileTreeEntriesParams,
} from "@/shared/ipc";

import { revealPath, requestServer } from "./transport";

const DOMAIN = "fileTree";

export function getFileTree(params: GetFileTreeParams = {}): Promise<FileTreeSnapshot> {
  return requestServer(DOMAIN, "get", params);
}

export function getFileTreeChildren(
  params: GetFileTreeChildrenParams,
): Promise<FileTreeChildrenSnapshot> {
  return requestServer(DOMAIN, "getChildren", params);
}

export function setFileTreeExpandedPaths(
  params: SetFileTreeExpandedPathsParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "setExpandedPaths", params);
}

export function createFileTreeEntry(params: CreateFileTreeEntryParams): Promise<boolean> {
  return requestServer(DOMAIN, "createEntry", params);
}

export function renameFileTreeEntry(params: RenameFileTreeEntryParams): Promise<boolean> {
  return requestServer(DOMAIN, "renameEntry", params);
}

export function moveFileTreeEntries(params: TransferFileTreeEntriesParams): Promise<boolean> {
  return requestServer(DOMAIN, "moveEntries", params);
}

export function copyFileTreeEntries(params: TransferFileTreeEntriesParams): Promise<boolean> {
  return requestServer(DOMAIN, "copyEntries", params);
}

export function deleteFileTreeEntries(params: DeleteFileTreeEntriesParams): Promise<boolean> {
  return requestServer(DOMAIN, "deleteEntries", params);
}

export function resolveFileTreePath(
  params: ResolveFileTreePathParams,
): Promise<FileTreeResolvedPath> {
  return requestServer(DOMAIN, "resolvePath", params);
}

export async function revealFileTreePath(params: ResolveFileTreePathParams): Promise<void> {
  const resolvedPath = await resolveFileTreePath(params);
  await revealPath(resolvedPath.path);
}

export type {
  CreateFileTreeEntryParams,
  DeleteFileTreeEntriesParams,
  FileTreeChildrenSnapshot,
  FileTreeResolvedPath,
  FileTreeSnapshot,
  GetFileTreeChildrenParams,
  GetFileTreeParams,
  RenameFileTreeEntryParams,
  ResolveFileTreePathParams,
  SetFileTreeExpandedPathsParams,
  TransferFileTreeEntriesParams,
} from "@/shared/ipc";
