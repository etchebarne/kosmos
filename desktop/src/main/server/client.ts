import net from "node:net";
import os from "node:os";
import path from "node:path";

import type { KosmosIpcDomain, KosmosIpcParams, KosmosServerResponse } from "../../shared/ipc";

type PendingRequest = {
  resolve(value: unknown): void;
  reject(error: Error): void;
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

export class KosmosServerClient {
  private activeRequests = 0;
  private buffer = "";
  private connecting: Promise<void> | undefined;
  private nextRequestId = 1;
  private shuttingDown = false;
  private socket: net.Socket | undefined;
  private readonly pending = new Map<number, PendingRequest>();
  private readonly idleWaiters = new Set<() => void>();

  constructor(readonly socketPath = defaultSocketPath()) {}

  async request<T = unknown>(
    domain: KosmosIpcDomain,
    action: string,
    params: KosmosIpcParams = {},
  ): Promise<T> {
    if (this.shuttingDown) {
      throw new Error("IPC client is shutting down");
    }

    this.activeRequests += 1;

    try {
      return await this.sendRequest(domain, action, params);
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

  private async sendRequest<T = unknown>(
    domain: KosmosIpcDomain,
    action: string,
    params: KosmosIpcParams,
  ): Promise<T> {
    await this.connect();

    const socket = this.socket;
    if (!socket || socket.destroyed) {
      throw new Error(`IPC client is not connected to ${this.socketPath}`);
    }

    const id = this.nextRequestId++;
    const payload = JSON.stringify({ type: "request", id, domain, action, params });

    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (value: unknown) => void, reject });

      socket.write(`${payload}\n`, "utf8", (error) => {
        if (!error) {
          return;
        }

        this.pending.delete(id);
        reject(error);
        this.disconnect();
      });
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

        socket.on("data", (chunk) => {
          try {
            this.handleData(chunk);
          } catch (caughtError: unknown) {
            this.failConnection(asError(caughtError));
          }
        });
        socket.on("error", (error) => this.rejectAll(error));
        socket.on("close", () => this.handleClose());

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

  private handleFrame(frame: string): void {
    const response = parseResponse(frame);
    const pending = this.pending.get(response.id);

    if (!pending) {
      return;
    }

    this.pending.delete(response.id);

    if (!response.ok) {
      pending.reject(new KosmosIpcRequestError(response.error.code, response.error.message));
      return;
    }

    pending.resolve(response.result);
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

function parseResponse(frame: string): KosmosServerResponse {
  const response: unknown = JSON.parse(frame);

  if (
    !response ||
    typeof response !== "object" ||
    !("type" in response) ||
    response.type !== "response" ||
    !("id" in response) ||
    typeof response.id !== "number" ||
    !Number.isSafeInteger(response.id) ||
    response.id < 0 ||
    !("ok" in response) ||
    typeof response.ok !== "boolean"
  ) {
    throw new Error("Invalid IPC response from server");
  }

  if (response.ok) {
    if (!("result" in response)) {
      throw new Error("Invalid successful IPC response from server");
    }
  } else if (
    !("error" in response) ||
    !response.error ||
    typeof response.error !== "object" ||
    !("code" in response.error) ||
    typeof response.error.code !== "string" ||
    !("message" in response.error) ||
    typeof response.error.message !== "string"
  ) {
    throw new Error("Invalid failed IPC response from server");
  }

  return response as KosmosServerResponse;
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
