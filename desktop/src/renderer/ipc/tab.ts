import type {
  ActivateTabParams,
  CloseTabParams,
  MoveTabParams,
  OpenTabParams,
  ReorderTabParams,
  SetTabKindParams,
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

export function closeTab(params: CloseTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "close", params);
}

export function reorderTab(params: ReorderTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "reorder", params);
}

export function moveTab(params: MoveTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "move", params);
}

export function splitTab(params: SplitTabParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "split", params);
}

export type { TabKind, TabSnapshot } from "@/shared/ipc";
