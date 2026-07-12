import { afterEach, describe, expect, test } from "bun:test";

import type { KosmosApi, KosmosIpcRequest } from "../src/shared/ipc";
import {
  RequestCancelledError,
  requestServer,
  type RequestCancellation,
} from "../src/renderer/ipc/transport";

const originalWindow = globalThis.window;

afterEach(() => {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: originalWindow,
  });
});

describe("cancellable renderer IPC", () => {
  test("emits one opaque cancellation key and returns a typed error", async () => {
    const cancellation = new TestCancellation();
    const cancelledKeys: string[] = [];
    let rejectRequest: ((error: Error) => void) | undefined;
    installApi({
      request: () =>
        new Promise((resolve) => {
          rejectRequest = (error) => {
            resolve({
              ok: false,
              error: {
                code: "language_servers.request_cancelled",
                message: error.message,
              },
            });
          };
        }),
      cancelRequest: (requestKey) => {
        cancelledKeys.push(requestKey);
        rejectRequest?.(new Error("request was cancelled"));
      },
    });

    const request = requestServer("languageServers", "hover", {}, cancellation);
    cancellation.cancel();
    cancellation.cancel();

    await expect(request).rejects.toBeInstanceOf(RequestCancelledError);
    expect(cancelledKeys).toHaveLength(1);
    expect(cancelledKeys[0]).toMatch(/^[0-9a-f-]{36}$/);
  });

  test("does not invoke IPC when already cancelled", async () => {
    const cancellation = new TestCancellation();
    cancellation.cancel();
    let requestCount = 0;
    installApi({
      request: async () => {
        requestCount += 1;
        return { ok: true, result: null };
      },
      cancelRequest: () => {},
    });

    await expect(
      requestServer("languageServers", "completion", {}, cancellation),
    ).rejects.toBeInstanceOf(RequestCancelledError);
    expect(requestCount).toBe(0);
  });
});

class TestCancellation implements RequestCancellation {
  isCancellationRequested = false;
  private readonly listeners = new Set<() => void>();

  onCancellationRequested(listener: () => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  cancel(): void {
    if (this.isCancellationRequested) {
      return;
    }
    this.isCancellationRequested = true;
    for (const listener of this.listeners) {
      listener();
    }
  }
}

function installApi(
  overrides: {
    request(request: KosmosIpcRequest): Promise<unknown>;
    cancelRequest(requestKey: string): void;
  },
): void {
  const api = {
    ...overrides,
  } as KosmosApi;
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { kosmos: api },
  });
}
