import type {
  ActivateWorkspaceParams,
  CloseWorkspaceParams,
  OpenWorkspaceParams,
  WorkspaceId,
  WorkspaceListSnapshot,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "workspace";

export function listWorkspaces(): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "list");
}

export function openWorkspace(path: string): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "open", { path } satisfies OpenWorkspaceParams);
}

export function activateWorkspace(workspaceId: WorkspaceId): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "activate", { workspaceId } satisfies ActivateWorkspaceParams);
}

export function closeWorkspace(workspaceId?: WorkspaceId | null): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "close", { workspaceId } satisfies CloseWorkspaceParams);
}

export type { WorkspaceListSnapshot, WorkspaceSnapshot } from "@/shared/ipc";
