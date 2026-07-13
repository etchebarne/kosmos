import type {
  OpenTerminalParams,
  ResizeTerminalParams,
  RestartTerminalParams,
  TerminalOutput,
  TerminalShell,
  TerminalTabParams,
  WriteTerminalInputParams,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "terminal";

export function listTerminalShells(): Promise<TerminalShell[]> {
  return requestServer(DOMAIN, "shells");
}

export function openTerminal(params: OpenTerminalParams): Promise<TerminalOutput> {
  return requestServer(DOMAIN, "open", params);
}

export function readTerminalOutput(params: TerminalTabParams): Promise<TerminalOutput> {
  return requestServer(DOMAIN, "read", params);
}

export function writeTerminalInput(params: WriteTerminalInputParams): Promise<boolean> {
  return requestServer(DOMAIN, "write", params);
}

export function resizeTerminal(params: ResizeTerminalParams): Promise<boolean> {
  return requestServer(DOMAIN, "resize", params);
}

export function restartTerminal(params: RestartTerminalParams): Promise<TerminalOutput> {
  return requestServer(DOMAIN, "restart", params);
}

export type {
  OpenTerminalParams,
  ResizeTerminalParams,
  RestartTerminalParams,
  TerminalOutput,
  TerminalShell,
  TerminalTabParams,
  WriteTerminalInputParams,
} from "@/shared/ipc";
