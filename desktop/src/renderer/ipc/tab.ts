import type {
  ActivateTabParams,
  CloseTabParams,
  CloseResultPayload,
  MoveTabParams,
  OpenTabParams,
  SetTabKindParams,
  ResolveCloseParams,
  SplitTabParams,
  WorkspaceListSnapshot,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "tab";

export function openTab(params: OpenTabParams = {}): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "open", params);
}

export function activateTab(params: ActivateTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "activate", params);
}

export function setTabKind(params: SetTabKindParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "setKind", params);
}

export function closeTab(params: CloseTabParams): Promise<CloseResultPayload> {
  return requestServer(DOMAIN, "close", params);
}

export function resolveTabClose(params: ResolveCloseParams): Promise<CloseResultPayload> {
  return requestServer(DOMAIN, "resolveClose", params);
}

export function moveTab(params: MoveTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "move", params);
}

export function splitTab(params: SplitTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "split", params);
}

export type { TabKind, TabLifecycle, TabSnapshot } from "@/shared/ipc";
