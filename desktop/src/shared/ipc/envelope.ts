export type KosmosIpcDomain = "workspace" | "pane" | "tab" | "fileTree" | "git" | "terminal";

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

export type KosmosServerResponse = {
  type: "response";
  id: number;
  ok: boolean;
  result?: unknown;
  error?: KosmosIpcError;
};

export type KosmosIpcRequestResult<T = unknown> =
  | { ok: true; result: T }
  | { ok: false; error: KosmosIpcError };

export type KosmosApi = {
  request<T = unknown>(request: KosmosIpcRequest): Promise<T>;
  getSocketPath(): Promise<string>;
  selectWorkspaceDirectory(): Promise<string | undefined>;
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  revealPath(path: string): Promise<void>;
};
