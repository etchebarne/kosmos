import type { WorkspaceId } from "./ids";

export type KosmosIpcDomain =
  | "workspace"
  | "pane"
  | "tab"
  | "fileTree"
  | "editor"
  | "git"
  | "terminal"
  | "settings";

export type KosmosIpcParams = Record<string, unknown>;

export type KosmosIpcRequest = {
  domain: KosmosIpcDomain;
  action: string;
  params?: KosmosIpcParams;
};

export type KosmosIpcError = {
  code: string;
  message: string;
};

export type KosmosServerResponse =
  | { type: "response"; id: number; ok: true; result: unknown }
  | { type: "response"; id: number; ok: false; error: KosmosIpcError };

export type KosmosServerNotification = {
  type: "notification";
  event: "workspaceChanged";
  workspaceIds: WorkspaceId[];
};

export type KosmosServerMessage = KosmosServerResponse | KosmosServerNotification;

export type KosmosIpcRequestResult<T = unknown> =
  | { ok: true; result: T }
  | { ok: false; error: KosmosIpcError };

export type KosmosApi = {
  request<T = unknown>(request: KosmosIpcRequest): Promise<T>;
  selectWorkspaceDirectory(): Promise<string | undefined>;
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  revealPath(path: string): Promise<void>;
  onFlushState(callback: () => Promise<void>): () => void;
  onWorkspaceChanged(callback: (workspaceIds: WorkspaceId[]) => void): () => void;
};
