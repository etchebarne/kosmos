import type { LspClient } from "../lib/lsp/client";
import type { Monaco } from "@monaco-editor/react";
import type { IDisposable } from "monaco-editor";

export type ServerStatus =
  | "starting"
  | "running"
  | "stopped"
  | "error"
  | "unavailable"
  | "installing";

export interface ServerAvailability {
  languageId: string;
  serverName: string;
  available: boolean;
}

export interface LspServerInfo {
  serverId: string;
  languageId: string;
  client: LspClient | null;
  status: ServerStatus;
  serverName: string;
  errorMessage: string | null;
  providerDisposables: IDisposable[];
}

export interface IndexProgress {
  serverName: string;
  title: string;
  message?: string;
  percentage?: number;
}

export interface LspState {
  // workspace path -> language -> server info
  servers: Record<string, Record<string, LspServerInfo>>;
  // workspace path -> availability info
  availability: Record<string, ServerAvailability[]>;
  // workspace path -> active indexing progress items
  indexProgress: Record<string, IndexProgress[]>;

  warmupWorkspace: (workspacePath: string) => Promise<void>;
  startServer: (
    workspacePath: string,
    languageId: string,
    filePath: string | null,
    monaco: Monaco,
  ) => Promise<LspClient | null>;
  getClient: (workspacePath: string, languageId: string) => LspClient | null;
  getCompanionClients: (workspacePath: string, languageId: string) => LspClient[];
  startCompanions: (
    workspacePath: string,
    primaryServerLang: string,
    filePath: string | null,
    monaco: Monaco,
  ) => Promise<void>;
  stopWorkspace: (workspacePath: string) => Promise<void>;
  checkAvailability: (workspacePath: string) => Promise<void>;
  installServer: (workspacePath: string, serverName: string) => Promise<void>;
}
