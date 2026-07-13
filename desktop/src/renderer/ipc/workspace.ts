import type {
  ActivateWorkspaceParams,
  CloseWorkspaceParams,
  CloseResultPayload,
  MoveWorkspaceParams,
  ResolveCloseParams,
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

export function moveWorkspace(params: MoveWorkspaceParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "move", params);
}

export function closeWorkspace(workspaceId?: WorkspaceId | null): Promise<CloseResultPayload> {
  return requestServer(DOMAIN, "close", { workspaceId } satisfies CloseWorkspaceParams);
}

export function resolveWorkspaceClose(params: ResolveCloseParams): Promise<CloseResultPayload> {
  return requestServer(DOMAIN, "resolveClose", params);
}

export function closeApplication(): Promise<CloseResultPayload> {
  return requestServer(DOMAIN, "closeApplication");
}

export type { WorkspaceListSnapshot, WorkspaceSnapshot } from "@/shared/ipc";
