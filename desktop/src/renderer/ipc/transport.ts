import type { KosmosIpcDomain, KosmosIpcParams } from "@/shared/ipc";
import { hasIpcErrorCode, requestResultValue } from "@/renderer/lib/errors";

export type RequestCancellation = {
  readonly isCancellationRequested: boolean;
  onCancellationRequested(listener: () => void): { dispose(): void };
};

export class RequestCancelledError extends Error {
  readonly code = "language_servers.request_cancelled";

  constructor() {
    super("language server request was cancelled");
    this.name = "RequestCancelledError";
  }
}

export function requestServer<T = unknown>(
  domain: KosmosIpcDomain,
  action: string,
  params?: KosmosIpcParams,
  cancellation?: RequestCancellation,
): Promise<T> {
  if (!cancellation) {
    return kosmosApi().request<T>({ domain, action, params }).then(requestResultValue);
  }
  return requestServerCancellable(domain, action, params, cancellation);
}

async function requestServerCancellable<T>(
  domain: KosmosIpcDomain,
  action: string,
  params: KosmosIpcParams | undefined,
  cancellation: RequestCancellation,
): Promise<T> {
  if (cancellation.isCancellationRequested) {
    throw new RequestCancelledError();
  }
  const requestKey = crypto.randomUUID();
  const api = kosmosApi();
  let cancelSent = false;
  const cancel = () => {
    if (cancelSent) {
      return;
    }
    cancelSent = true;
    api.cancelRequest(requestKey);
  };
  const subscription = cancellation.onCancellationRequested(cancel);
  try {
    const request = api.request<T>({ domain, action, params, requestKey });
    if (cancellation.isCancellationRequested) {
      cancel();
    }
    return requestResultValue(await request);
  } catch (error) {
    if (isRequestCancelledError(error)) {
      throw new RequestCancelledError();
    }
    throw error;
  } finally {
    subscription.dispose();
  }
}

export function isRequestCancelledError(error: unknown): boolean {
  return (
    error instanceof RequestCancelledError ||
    hasIpcErrorCode(error, "language_servers.request_cancelled")
  );
}

export function selectWorkspaceDirectory(): Promise<string | undefined> {
  return kosmosApi().selectWorkspaceDirectory();
}

export function minimizeWindow(): Promise<void> {
  return kosmosApi().minimizeWindow();
}

export function toggleMaximizeWindow(): Promise<void> {
  return kosmosApi().toggleMaximizeWindow();
}

export function closeWindow(): Promise<void> {
  return kosmosApi().closeWindow();
}

export function revealPath(path: string): Promise<void> {
  return kosmosApi().revealPath(path);
}

function kosmosApi() {
  if (!window.kosmos) {
    throw new Error("Electron preload did not expose the Kosmos IPC API.");
  }

  return window.kosmos;
}
