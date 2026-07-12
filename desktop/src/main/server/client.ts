import net from "node:net";
import os from "node:os";
import path from "node:path";

import type {
  KosmosIpcDomain,
  KosmosIpcParams,
  KosmosServerMessage,
  KosmosServerNotification,
  WorkspaceId,
} from "../../shared/ipc";

type PendingRequest = {
  resolve(value: unknown): void;
  reject(error: Error): void;
  cleanup(): void;
};

const MAX_RESPONSE_FRAME_CHARS = 64 * 1024 * 1024;

export class KosmosIpcRequestError extends Error {
  constructor(
    readonly code: string,
    readonly messageWithoutCode: string,
  ) {
    super(`${code}: ${messageWithoutCode}`);
    this.name = "KosmosIpcRequestError";
  }
}

export class RequestCancelledError extends KosmosIpcRequestError {
  constructor() {
    super("language_servers.request_cancelled", "language server request was cancelled");
    this.name = "RequestCancelledError";
  }
}

export class KosmosServerClient {
  private activeRequests = 0;
  private buffer = "";
  private connecting: Promise<void> | undefined;
  private nextRequestId = 1;
  private shuttingDown = false;
  private hasConnected = false;
  private socket: net.Socket | undefined;
  private readonly pending = new Map<number, PendingRequest>();
  private readonly idleWaiters = new Set<() => void>();
  private readonly workspaceChangedListeners = new Set<(workspaceIds: WorkspaceId[]) => void>();
  private readonly notificationListeners = new Set<(notification: KosmosServerNotification) => void>();
  private readonly reconnectedListeners = new Set<() => void>();

  constructor(readonly socketPath = defaultSocketPath()) {}

  async request<T = unknown>(
    domain: KosmosIpcDomain,
    action: string,
    params: KosmosIpcParams = {},
    signal?: AbortSignal,
  ): Promise<T> {
    if (this.shuttingDown) {
      throw new Error("IPC client is shutting down");
    }

    this.activeRequests += 1;

    try {
      return await this.sendRequest(domain, action, params, signal);
    } finally {
      this.activeRequests -= 1;
      this.notifyIdle();
    }
  }

  async flushPersistence(): Promise<void> {
    this.shuttingDown = true;
    await this.waitForIdle();
    await this.sendRequest("workspace", "flush", {});
  }

  async acknowledgeApplyEdit(
    id: number,
    token: string,
    applied: boolean,
    failureReason?: string,
  ): Promise<void> {
    await this.connect();
    const socket = this.socket;
    if (!socket || socket.destroyed) {
      throw new Error(`IPC client is not connected to ${this.socketPath}`);
    }
    const payload = JSON.stringify({
      type: "applyEditAck",
      id,
      token,
      applied,
      failureReason: failureReason ?? null,
    });
    await new Promise<void>((resolve, reject) => {
      socket.write(`${payload}\n`, "utf8", (error) => error ? reject(error) : resolve());
    });
  }

  onWorkspaceChanged(listener: (workspaceIds: WorkspaceId[]) => void): () => void {
    this.workspaceChangedListeners.add(listener);
    return () => this.workspaceChangedListeners.delete(listener);
  }

  onNotification(listener: (notification: KosmosServerNotification) => void): () => void {
    this.notificationListeners.add(listener);
    return () => this.notificationListeners.delete(listener);
  }

  onReconnected(listener: () => void): () => void {
    this.reconnectedListeners.add(listener);
    return () => this.reconnectedListeners.delete(listener);
  }

  private async sendRequest<T = unknown>(
    domain: KosmosIpcDomain,
    action: string,
    params: KosmosIpcParams,
    signal?: AbortSignal,
  ): Promise<T> {
    if (signal?.aborted) {
      throw new RequestCancelledError();
    }
    await this.connect();
    if (signal?.aborted) {
      throw new RequestCancelledError();
    }

    const socket = this.socket;
    if (!socket || socket.destroyed) {
      throw new Error(`IPC client is not connected to ${this.socketPath}`);
    }

    const id = this.allocateRequestId();
    const payload = JSON.stringify({ type: "request", id, domain, action, params });

    return new Promise<T>((resolve, reject) => {
      const onAbort = () => {
        const pending = this.pending.get(id);
        if (!pending) {
          return;
        }
        this.pending.delete(id);
        pending.cleanup();
        socket.write(`${JSON.stringify({ type: "cancel", id })}\n`);
        reject(new RequestCancelledError());
      };
      const cleanup = () => signal?.removeEventListener("abort", onAbort);
      this.pending.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
        cleanup,
      });

      socket.write(`${payload}\n`, "utf8", (error) => {
        if (!error) {
          return;
        }

        const pending = this.pending.get(id);
        this.pending.delete(id);
        pending?.cleanup();
        reject(error);
        this.disconnect();
      });
      signal?.addEventListener("abort", onAbort, { once: true });
      if (signal?.aborted) {
        onAbort();
      }
    });
  }

  disconnect(): void {
    this.socket?.destroy();
    this.socket = undefined;
    this.rejectAll(new Error("IPC connection closed"));
  }

  private async connect(): Promise<void> {
    if (this.socket && !this.socket.destroyed) {
      return;
    }

    if (this.connecting) {
      return this.connecting;
    }

    this.connecting = new Promise<void>((resolve, reject) => {
      const socket = net.createConnection(this.socketPath);
      socket.setEncoding("utf8");

      const onConnect = () => {
        socket.off("error", onConnectError);
        this.socket = socket;
        this.buffer = "";
        const reconnected = this.hasConnected;
        this.hasConnected = true;

        socket.on("data", (chunk) => {
          try {
            this.handleData(chunk);
          } catch (caughtError: unknown) {
            this.failConnection(asError(caughtError));
          }
        });
        socket.on("error", (error) => this.rejectAll(error));
        socket.on("close", () => this.handleClose());

        if (reconnected) {
          for (const listener of this.reconnectedListeners) {
            listener();
          }
        }

        resolve();
      };

      const onConnectError = (error: Error) => {
        socket.off("connect", onConnect);
        reject(error);
      };

      socket.once("connect", onConnect);
      socket.once("error", onConnectError);
    }).finally(() => {
      this.connecting = undefined;
    });

    return this.connecting;
  }

  private handleData(chunk: string | Buffer): void {
    this.buffer += chunk.toString();

    if (this.buffer.length > MAX_RESPONSE_FRAME_CHARS && !this.buffer.includes("\n")) {
      throw new Error(`IPC response exceeds the ${MAX_RESPONSE_FRAME_CHARS}-character limit`);
    }

    const frames = this.buffer.split("\n");
    this.buffer = frames.pop() ?? "";

    for (const frame of frames) {
      if (frame.length > MAX_RESPONSE_FRAME_CHARS) {
        throw new Error(`IPC response exceeds the ${MAX_RESPONSE_FRAME_CHARS}-character limit`);
      }

      const trimmed = frame.trim();
      if (trimmed.length > 0) {
        this.handleFrame(trimmed);
      }
    }
  }

  private allocateRequestId(): number {
    for (let attempt = 0; attempt <= this.pending.size; attempt += 1) {
      const id = this.nextRequestId;
      this.nextRequestId = id === Number.MAX_SAFE_INTEGER ? 1 : id + 1;
      if (!this.pending.has(id)) {
        return id;
      }
    }
    throw new Error("IPC request ID space is exhausted");
  }

  private handleFrame(frame: string): void {
    const message = parseServerMessage(frame);

    if (message.type === "notification") {
      for (const listener of this.notificationListeners) {
        listener(message);
      }
      if (message.event === "workspaceChanged") {
        for (const listener of this.workspaceChangedListeners) {
          listener(message.workspaceIds);
        }
      }
      return;
    }

    const pending = this.pending.get(message.id);

    if (!pending) {
      return;
    }

    this.pending.delete(message.id);
    pending.cleanup();

    if (!message.ok) {
      pending.reject(new KosmosIpcRequestError(message.error.code, message.error.message));
      return;
    }

    pending.resolve(message.result);
  }

  private handleClose(): void {
    this.socket = undefined;
    this.rejectAll(new Error("IPC server closed the connection"));
  }

  private failConnection(error: Error): void {
    this.socket?.destroy();
    this.socket = undefined;
    this.rejectAll(error);
  }

  private rejectAll(error: Error): void {
    for (const request of this.pending.values()) {
      request.cleanup();
      request.reject(error);
    }

    this.pending.clear();
  }

  private waitForIdle(): Promise<void> {
    if (this.activeRequests === 0) {
      return Promise.resolve();
    }

    return new Promise((resolve) => this.idleWaiters.add(resolve));
  }

  private notifyIdle(): void {
    if (this.activeRequests !== 0) {
      return;
    }

    for (const resolve of this.idleWaiters) {
      resolve();
    }

    this.idleWaiters.clear();
  }
}

export function defaultSocketPath(): string {
  const socketPath = process.env.KOSMOS_SOCKET;
  if (socketPath && socketPath.length > 0) {
    return socketPath;
  }

  const runtimeDir = process.env.XDG_RUNTIME_DIR || os.tmpdir();
  return path.join(runtimeDir, "kosmos", "server.sock");
}

function parseServerMessage(frame: string): KosmosServerMessage {
  const message: unknown = JSON.parse(frame);

  if (
    !message ||
    typeof message !== "object" ||
    !("type" in message) ||
    (message.type !== "response" && message.type !== "notification")
  ) {
    throw new Error("Invalid IPC message from server");
  }

  if (message.type === "notification") {
    if (!("event" in message) || typeof message.event !== "string") {
      throw new Error("Invalid IPC notification from server");
    }
    if (message.event === "workspaceChanged") {
      if (
        !("workspaceIds" in message) ||
        !Array.isArray(message.workspaceIds) ||
        !message.workspaceIds.every(isNonNegativeSafeInteger)
      ) {
        throw new Error("Invalid workspace notification from server");
      }
      return message as KosmosServerMessage;
    }
    if (
      message.event === "languageServerStatusChanged" ||
      message.event === "languageServerLogAvailable"
    ) {
      if (!("serverId" in message) || typeof message.serverId !== "string") {
        throw new Error("Invalid language server notification from server");
      }
      return message as KosmosServerMessage;
    }
    if (message.event === "languageServerDiagnosticsChanged") {
      if (!isDiagnosticsNotification(message)) {
        throw new Error("Invalid language server diagnostics notification from server");
      }
      return message as KosmosServerMessage;
    }
    if (message.event === "languageServerDiagnosticsResync") {
      return message as KosmosServerMessage;
    }
    if (message.event === "languageServerApplyEdit") {
      if (
        !("id" in message) ||
        !isNonNegativeSafeInteger(message.id) ||
        !("token" in message) ||
        typeof message.token !== "string" ||
        !("edit" in message) ||
        !isStagedWorkspaceEdit(message.edit)
      ) {
        throw new Error("Invalid language server apply-edit notification from server");
      }
      return message as KosmosServerMessage;
    }
    if (message.event === "languageServerApplyEditCancelled") {
      if (
        !("id" in message) ||
        !isNonNegativeSafeInteger(message.id) ||
        !("token" in message) ||
        typeof message.token !== "string"
      ) {
        throw new Error("Invalid language server apply-edit cancellation from server");
      }
      return message as KosmosServerMessage;
    }
    throw new Error("Unsupported IPC notification from server");
  }

  if (
    !("id" in message) ||
    typeof message.id !== "number" ||
    !Number.isSafeInteger(message.id) ||
    message.id < 0 ||
    !("ok" in message) ||
    typeof message.ok !== "boolean"
  ) {
    throw new Error("Invalid IPC response from server");
  }

  if (message.ok) {
    if (!("result" in message)) {
      throw new Error("Invalid successful IPC response from server");
    }
  } else if (
    !("error" in message) ||
    !message.error ||
    typeof message.error !== "object" ||
    !("code" in message.error) ||
    typeof message.error.code !== "string" ||
    !("message" in message.error) ||
    typeof message.error.message !== "string"
  ) {
    throw new Error("Invalid failed IPC response from server");
  }

  return message as KosmosServerMessage;
}

function isStagedWorkspaceEdit(value: unknown): boolean {
  return Boolean(
    value &&
      typeof value === "object" &&
      "transactionId" in value &&
      isNonNegativeSafeInteger(value.transactionId) &&
      "authorization" in value &&
      typeof value.authorization === "string" &&
      value.authorization.length === 64 &&
      "documents" in value &&
      Array.isArray(value.documents) &&
      value.documents.length <= 64 &&
      value.documents.every((document) =>
        Boolean(
          document &&
            typeof document === "object" &&
            "workspaceId" in document &&
            isNonNegativeSafeInteger(document.workspaceId) &&
            "path" in document &&
            typeof document.path === "string" &&
            "originalPath" in document &&
            typeof document.originalPath === "string" &&
            "originalText" in document &&
            typeof document.originalText === "string" &&
            "newText" in document &&
            typeof document.newText === "string" &&
            "generation" in document &&
            (document.generation === null || isNonNegativeSafeInteger(document.generation)) &&
            "version" in document &&
            (document.version === null ||
              (typeof document.version === "number" && Number.isSafeInteger(document.version))),
        ),
      ) &&
      "operations" in value &&
      Array.isArray(value.operations) &&
      value.operations.length <= 4096 &&
      value.operations.every((operation) =>
        Boolean(
          operation &&
            typeof operation === "object" &&
            "kind" in operation &&
            typeof operation.kind === "string",
        ),
      ),
  );
}

function isNonNegativeSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isDiagnosticsNotification(message: object): boolean {
  return (
    "workspaceId" in message &&
    isNonNegativeSafeInteger(message.workspaceId) &&
    "path" in message &&
    typeof message.path === "string" &&
    "serverId" in message &&
    typeof message.serverId === "string" &&
    "generation" in message &&
    isNonNegativeSafeInteger(message.generation) &&
    "version" in message &&
    typeof message.version === "number" &&
    Number.isSafeInteger(message.version) &&
    "diagnostics" in message &&
    Array.isArray(message.diagnostics)
  );
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
