import net from "node:net";
import os from "node:os";
import path from "node:path";

import type { KosmosIpcDomain, KosmosIpcParams, KosmosServerResponse } from "../shared/ipc";

type PendingRequest = {
  resolve(value: unknown): void;
  reject(error: Error): void;
};

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
  private buffer = "";
  private connecting: Promise<void> | undefined;
  private nextRequestId = 1;
  private socket: net.Socket | undefined;
  private readonly pending = new Map<number, PendingRequest>();

  constructor(readonly socketPath = defaultSocketPath()) {}

  async request<T = unknown>(
    domain: KosmosIpcDomain,
    action: string,
    params: KosmosIpcParams = {},
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

        socket.on("data", (chunk) => this.handleData(chunk));
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

    const frames = this.buffer.split("\n");
    this.buffer = frames.pop() ?? "";

    for (const frame of frames) {
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
      const error = response.error ?? {
        code: "ipc.server_error",
        message: "server returned an IPC error",
      };
      pending.reject(new KosmosIpcRequestError(error.code, error.message));
      return;
    }

    pending.resolve(response.result);
  }

  private handleClose(): void {
    this.socket = undefined;
    this.rejectAll(new Error("IPC server closed the connection"));
  }

  private rejectAll(error: Error): void {
    for (const request of this.pending.values()) {
      request.reject(error);
    }

    this.pending.clear();
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
  const response = JSON.parse(frame) as Partial<KosmosServerResponse>;

  if (response.type !== "response" || typeof response.id !== "number") {
    throw new Error("Invalid IPC response from server");
  }

  return response as KosmosServerResponse;
}
