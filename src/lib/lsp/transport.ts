import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params?: unknown;
}

interface JsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: unknown;
}

interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

type JsonRpcMessage = JsonRpcRequest | JsonRpcNotification | JsonRpcResponse;

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
  timer: ReturnType<typeof setTimeout>;
  method: string;
}

interface StatusPayload {
  status: string;
  error?: string | null;
}

const REQUEST_TIMEOUT_MS = 30_000;

interface CancellablePromise<T> extends Promise<T> {
  cancel: () => void;
}

/** JSON-RPC over invoke("lsp_send") + Tauri events. */
export class TauriLspTransport {
  private serverId: string;
  private requestId = 0;
  private pendingRequests = new Map<number, PendingRequest>();
  private notificationHandlers = new Map<string, ((params: unknown) => void)[]>();
  private requestHandlers = new Map<string, (params: unknown) => unknown>();
  private stoppedCallbacks: ((error?: string | null) => void)[] = [];
  private unlistenMessage: UnlistenFn | null = null;
  private unlistenStatus: UnlistenFn | null = null;
  private disposed = false;

  constructor(serverId: string) {
    this.serverId = serverId;
  }

  async connect(): Promise<void> {
    this.unlistenMessage = await listen<string>(`lsp-message:${this.serverId}`, (event) => {
      this.handleMessage(event.payload);
    });

    this.unlistenStatus = await listen<StatusPayload>(`lsp-status:${this.serverId}`, (event) => {
      if (event.payload.status === "stopped") {
        this.handleServerStopped(event.payload.error);
      }
    });
  }

  /** Send a JSON-RPC message to the server (no-op if disposed). */
  private send(message: object): Promise<void> {
    if (this.disposed) return Promise.resolve();
    return invoke("lsp_send", {
      serverId: this.serverId,
      message: JSON.stringify(message),
    }) as Promise<void>;
  }

  sendRequest<R>(method: string, params?: unknown, timeoutMs?: number): CancellablePromise<R> {
    if (this.disposed) {
      const p = Promise.reject(new Error("Transport disposed")) as CancellablePromise<R>;
      p.cancel = () => {};
      return p;
    }

    const id = ++this.requestId;
    const request: JsonRpcRequest = { jsonrpc: "2.0", id, method, params };
    const timeout = timeoutMs ?? REQUEST_TIMEOUT_MS;

    let cancelFn = () => {};

    const promise = new Promise<R>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingRequests.delete(id);
        reject(new Error(`LSP request '${method}' timed out after ${timeout}ms`));
      }, timeout);

      this.pendingRequests.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timer,
        method,
      });

      cancelFn = () => {
        if (this.pendingRequests.has(id)) {
          clearTimeout(timer);
          this.pendingRequests.delete(id);
          reject(new Error(`LSP request '${method}' cancelled`));
          this.sendNotification("$/cancelRequest", { id });
        }
      };

      this.send(request).catch((err) => {
        clearTimeout(timer);
        this.pendingRequests.delete(id);
        reject(err);
      });
    }) as CancellablePromise<R>;

    promise.cancel = cancelFn;
    return promise;
  }

  sendNotification(method: string, params?: unknown): void {
    const notification: JsonRpcNotification = { jsonrpc: "2.0", method, params };
    this.send(notification).catch((err) => {
      console.warn(`[kosmos:lsp] Notification '${method}' delivery failed:`, err);
    });
  }

  onNotification(method: string, handler: (params: unknown) => void): void {
    const handlers = this.notificationHandlers.get(method) ?? [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);
  }

  /** Register a handler for server-initiated requests (server sends id + method, expects a response). */
  onRequest(method: string, handler: (params: unknown) => unknown): void {
    this.requestHandlers.set(method, handler);
  }

  /** Register a callback invoked when the server stops (crash, EOF, or write failure). */
  onServerStopped(callback: (error?: string | null) => void): void {
    this.stoppedCallbacks.push(callback);
  }

  dispose(): void {
    this.disposed = true;
    this.unlistenMessage?.();
    this.unlistenStatus?.();

    for (const [, pending] of this.pendingRequests) {
      clearTimeout(pending.timer);
      pending.reject(new Error("Transport disposed"));
    }
    this.pendingRequests.clear();
    this.notificationHandlers.clear();
    this.requestHandlers.clear();
    this.stoppedCallbacks = [];
  }

  /** Send a successful JSON-RPC response to a server-initiated request. */
  private respondToRequest(id: number, result: unknown): void {
    this.send({ jsonrpc: "2.0", id, result } satisfies JsonRpcResponse).catch(() => {});
  }

  /** Send a JSON-RPC error response to a server-initiated request. */
  private respondToRequestWithError(id: number, code: number, message: string): void {
    this.send({
      jsonrpc: "2.0",
      id,
      error: { code, message },
    } satisfies JsonRpcResponse).catch(() => {});
  }

  private handleMessage(raw: string): void {
    let msg: JsonRpcMessage;
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }

    if ("id" in msg && msg.id != null && !("method" in msg)) {
      const response = msg as JsonRpcResponse;
      const pending = this.pendingRequests.get(response.id);
      if (pending) {
        clearTimeout(pending.timer);
        this.pendingRequests.delete(response.id);
        if (response.error) {
          // -32800 (RequestCancelled) follows our $/cancelRequest; expected.
          if (response.error.code !== -32800) {
            pending.reject(
              new Error(`LSP error ${response.error.code}: ${response.error.message}`),
            );
          } else {
            pending.reject(new Error(`LSP request '${pending.method}' cancelled by server`));
          }
        } else {
          pending.resolve(response.result);
        }
      }
      return;
    }

    // Server requests must always get a response; unhandled methods return -32601.
    if ("method" in msg && "id" in msg && msg.id != null) {
      const request = msg as JsonRpcRequest;
      const handler = this.requestHandlers.get(request.method);
      if (handler) {
        this.respondToRequest(request.id, handler(request.params));
      } else {
        this.respondToRequestWithError(
          request.id,
          -32601,
          `Method not supported: ${request.method}`,
        );
      }
      return;
    }

    if ("method" in msg) {
      const notification = msg as JsonRpcNotification;
      const handlers = this.notificationHandlers.get(notification.method);
      if (handlers) {
        for (const handler of handlers) {
          handler(notification.params);
        }
      }
    }
  }

  private handleServerStopped(error?: string | null): void {
    const message = error ? `Language server stopped: ${error}` : "Language server stopped";
    for (const [, pending] of this.pendingRequests) {
      clearTimeout(pending.timer);
      pending.reject(new Error(message));
    }
    this.pendingRequests.clear();

    for (const cb of this.stoppedCallbacks) {
      cb(error);
    }
  }
}
