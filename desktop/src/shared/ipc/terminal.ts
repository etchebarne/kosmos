import type { TabId, WorkspaceId } from "./ids";

export type OpenTerminalParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
  columns: number;
  rows: number;
};

export type TerminalTabParams = {
  workspaceId?: WorkspaceId | null;
  tabId: TabId;
};

export type WriteTerminalInputParams = TerminalTabParams & {
  data: string;
};

export type ResizeTerminalParams = TerminalTabParams & {
  columns: number;
  rows: number;
};

export type RestartTerminalParams = ResizeTerminalParams & {
  shell: string;
};

export type TerminalShell = {
  name: string;
  path: string;
  isDefault: boolean;
};

export type TerminalOutput = {
  output: string;
  truncated: boolean;
  exited: boolean;
  exitCode?: number | null;
  signal?: string | null;
};
