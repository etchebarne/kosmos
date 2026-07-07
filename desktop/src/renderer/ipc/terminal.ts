import type {
  OpenTerminalParams,
  ResizeTerminalParams,
  TerminalOutput,
  TerminalTabParams,
  WriteTerminalInputParams,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "terminal";

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

export function closeTerminal(params: TerminalTabParams): Promise<boolean> {
  return requestServer(DOMAIN, "close", params);
}

export type {
  OpenTerminalParams,
  ResizeTerminalParams,
  TerminalOutput,
  TerminalTabParams,
  WriteTerminalInputParams,
} from "@/shared/ipc";
