import type { WorkspaceId } from "./ids";
import type { LanguageServerDiagnosticsChanged, StagedWorkspaceEdit } from "./language-servers";

export type KosmosIpcDomain =
  | "workspace"
  | "pane"
  | "tab"
  | "fileTree"
  | "formatters"
  | "editor"
  | "git"
  | "search"
  | "terminal"
  | "settings"
  | "languageServers"
  | "window";

export type KosmosIpcParams = Record<string, unknown>;

export type KosmosIpcRequest = {
  domain: KosmosIpcDomain;
  action: string;
  params?: KosmosIpcParams;
  requestKey?: string;
};

export type KosmosIpcError = {
  code: string;
  message: string;
};

export type KosmosServerResponse =
  | { type: "response"; id: number; ok: true; result: unknown }
  | { type: "response"; id: number; ok: false; error: KosmosIpcError };

export type KosmosServerNotification =
  | { type: "notification"; event: "workspaceChanged"; workspaceIds: WorkspaceId[] }
  | ({ type: "notification"; event: "languageServerDiagnosticsChanged" } &
      LanguageServerDiagnosticsChanged)
  | { type: "notification"; event: "languageServerDiagnosticsResync" }
  | { type: "notification"; event: "languageServerStatusChanged"; serverId: string }
  | { type: "notification"; event: "languageServerLogAvailable"; serverId: string }
  | {
      type: "notification";
      event: "languageServerApplyEdit";
      id: number;
      token: string;
      edit: StagedWorkspaceEdit;
    }
  | {
      type: "notification";
      event: "languageServerApplyEditCancelled";
      id: number;
      token: string;
    };

export type KosmosServerMessage = KosmosServerResponse | KosmosServerNotification;

export type KosmosIpcRequestResult<T = unknown> =
  | { ok: true; result: T }
  | { ok: false; error: KosmosIpcError };

export type KosmosApi = {
  request<T = unknown>(request: KosmosIpcRequest): Promise<T>;
  cancelRequest(requestKey: string): void;
  acknowledgeServerApplyEdit(
    id: number,
    token: string,
    applied: boolean,
    failureReason?: string,
  ): void;
  pendingServerApplyEdits(): Promise<
    Array<Extract<KosmosServerNotification, { event: "languageServerApplyEdit" }>>
  >;
  selectWorkspaceDirectory(): Promise<string | undefined>;
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  setZoomLevel(zoomLevel: number): Promise<void>;
  revealPath(path: string): Promise<void>;
  onFlushState(callback: () => Promise<void>): () => void;
  onZoomLevelChanged(callback: (zoomLevel: number) => void): () => void;
  onWorkspaceChanged(callback: (workspaceIds: WorkspaceId[]) => void): () => void;
  onServerNotification(callback: (notification: KosmosServerNotification) => void): () => void;
  onServerReconnected(callback: () => void): () => void;
};
