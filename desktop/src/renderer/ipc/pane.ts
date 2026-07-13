import type {
  ActivatePaneParams,
  MovePaneParams,
  ResizeSplitParams,
  SplitPaneParams,
  WorkspaceListSnapshot,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "pane";

export function splitPane(params: SplitPaneParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "split", params);
}

export function activatePane(params: ActivatePaneParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "activate", params);
}

export function movePane(params: MovePaneParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "move", params);
}

export function resizeSplit(params: ResizeSplitParams): Promise<WorkspaceListSnapshot> {
  return requestServer(DOMAIN, "resize", params);
}

export type { PaneNodeSnapshot, PaneSnapshot, SplitAxis } from "@/shared/ipc";
