import { afterEach, describe, expect, test } from "bun:test";

import {
  disposeEditorBuffer,
  flushEditorBuffer,
  getOrCreateEditorBuffer,
  initializeEditorBufferRecovery,
  queueEditorBufferSynchronization,
  setLanguageDocumentAttacher,
} from "@/renderer/lib/editor-buffers";
import type { KosmosApi, KosmosIpcRequest } from "@/shared/ipc";

type MockModel = {
  disposed: boolean;
  value: string;
  version: number;
  getValue(): string;
  getVersionId(): number;
  isDisposed(): boolean;
  dispose(): void;
  setValue(value: string): void;
};

setLanguageDocumentAttacher(() => ({ dispose() {} }));
const originalWindow = globalThis.window;

afterEach(() => {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: originalWindow,
  });
});

describe("document lifecycle", () => {
  test("rapid editor changes coalesce before asynchronous session synchronization", async () => {
    const requests: KosmosIpcRequest[] = [];
    installApi((request) => {
      requests.push(request);
      return sessionResult((request.params as { content: string; revision: number }).content,
        (request.params as { content: string; revision: number }).revision);
    });
    const model = mockModel("before");
    const buffer = getOrCreateEditorBuffer(901, 1, "document.txt", "before", () => model as never);

    model.setValue("first");
    queueEditorBufferSynchronization(buffer);
    model.setValue("second");
    queueEditorBufferSynchronization(buffer);
    await flushEditorBuffer(buffer);

    expect(requests).toHaveLength(1);
    expect(requests[0]?.action).toBe("changeSession");
    expect((requests[0]?.params as { content: string }).content).toBe("second");
    disposeEditorBuffer(901, 1);
  });

  test("a stale acknowledgement cannot mark a newer local buffer clean", async () => {
    let calls = 0;
    installApi(() => {
      calls += 1;
      return calls === 1
        ? sessionResult("server", 4, false, "server")
        : sessionResult("newer local text", 5);
    });
    const model = mockModel("before");
    const buffer = getOrCreateEditorBuffer(902, 1, "document.txt", "before", () => model as never);

    model.setValue("newer local text");
    queueEditorBufferSynchronization(buffer);
    await flushEditorBuffer(buffer);

    expect(buffer.savedContent).toBe("before");
    expect(buffer.model.getValue()).toBe("newer local text");
    expect(calls).toBe(2);
    disposeEditorBuffer(902, 1);
  });

  test("typing queues IPC without awaiting it", () => {
    installApi(() => new Promise(() => {}));
    const model = mockModel("before");
    const buffer = getOrCreateEditorBuffer(903, 1, "document.txt", "before", () => model as never);

    model.setValue("typed immediately");
    queueEditorBufferSynchronization(buffer);

    expect(buffer.model.getValue()).toBe("typed immediately");
    expect(buffer.session.synchronization).not.toBeNull();
    disposeEditorBuffer(903, 1);
  });

  test("sidecar recovery preserves current text and the saved baseline", async () => {
    const requests: KosmosIpcRequest[] = [];
    let reconnect: ((generation: number) => void) | undefined;
    installApi(
      (request) => {
        requests.push(request);
        const params = request.params as { content: string; revision: number; savedContent: string };
        return sessionResult(params.content, params.revision, true, params.savedContent);
      },
      (listener) => {
        reconnect = listener;
      },
    );
    initializeEditorBufferRecovery();
    const model = mockModel("saved");
    const buffer = getOrCreateEditorBuffer(904, 1, "document.txt", "saved", () => model as never);
    model.setValue("unsaved");

    reconnect?.(1);
    await flushEditorBuffer(buffer);

    expect(requests).toHaveLength(1);
    expect(requests[0]?.action).toBe("restoreSession");
    expect(requests[0]?.params).toMatchObject({
      content: "unsaved",
      savedContent: "saved",
    });
    expect(buffer.savedContent).toBe("saved");
    expect(buffer.model.getValue()).toBe("unsaved");
    disposeEditorBuffer(904, 1);
  });

  test("sidecar recovery advances beyond a stale server session", async () => {
    const requests: KosmosIpcRequest[] = [];
    let reconnect: ((generation: number) => void) | undefined;
    let recovered: (() => void) | undefined;
    installApi(
      (request) => {
        requests.push(request);
        const params = request.params as { content: string; revision: number; savedContent: string };
        return requests.length === 1
          ? sessionResult("server text", params.revision + 2, false, params.savedContent)
          : sessionResult(params.content, params.revision, true, params.savedContent);
      },
      (listener) => {
        reconnect = listener;
      },
      () => recovered?.(),
    );
    const recoveredPromise = new Promise<void>((resolve) => {
      recovered = resolve;
    });
    initializeEditorBufferRecovery();
    const model = mockModel("saved");
    const buffer = getOrCreateEditorBuffer(905, 1, "document.txt", "saved", () => model as never);
    model.setValue("local text");

    reconnect?.(1);
    await recoveredPromise;

    expect(requests).toHaveLength(2);
    expect((requests[1]?.params as { revision: number }).revision).toBeGreaterThan(
      (requests[0]?.params as { revision: number }).revision,
    );
    expect(buffer.model.getValue()).toBe("local text");
    expect(buffer.savedContent).toBe("saved");
    disposeEditorBuffer(905, 1);
  });

  test("sidecar recovery includes edits made while restoration is in flight", async () => {
    const requests: KosmosIpcRequest[] = [];
    let reconnect: ((generation: number) => void) | undefined;
    let resolveFirst: (() => void) | undefined;
    let recovered: (() => void) | undefined;
    installApi(
      (request) => {
        requests.push(request);
        const params = request.params as { content: string; revision: number; savedContent: string };
        if (requests.length === 1) {
          return new Promise((resolve) => {
            resolveFirst = () =>
              resolve(sessionResult(params.content, params.revision, true, params.savedContent));
          });
        }
        return sessionResult(params.content, params.revision, true, params.savedContent);
      },
      (listener) => {
        reconnect = listener;
      },
      () => recovered?.(),
    );
    const recoveredPromise = new Promise<void>((resolve) => {
      recovered = resolve;
    });
    initializeEditorBufferRecovery();
    const model = mockModel("saved");
    const buffer = getOrCreateEditorBuffer(906, 1, "document.txt", "saved", () => model as never);
    model.setValue("first edit");

    reconnect?.(1);
    while (!resolveFirst) {
      await Promise.resolve();
    }
    model.setValue("edit during recovery");
    queueEditorBufferSynchronization(buffer);
    resolveFirst?.();
    await recoveredPromise;

    expect(requests).toHaveLength(2);
    expect((requests[1]?.params as { content: string }).content).toBe("edit during recovery");
    expect(buffer.model.getValue()).toBe("edit during recovery");
    expect(buffer.savedContent).toBe("saved");
    disposeEditorBuffer(906, 1);
  });
});

function installApi(
  request: (request: KosmosIpcRequest) => Promise<unknown> | unknown,
  onReconnect?: (listener: (generation: number) => void) => void,
  onRecoveryComplete?: (error?: string) => void,
): void {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      kosmos: {
        request: async <T>(message: KosmosIpcRequest) => ({
          ok: true,
          result: await request(message),
        }) as Awaited<ReturnType<KosmosApi["request"]>> as T,
        onServerReconnected(listener: (generation: number) => void) {
          onReconnect?.(listener);
          return () => {};
        },
        completeServerRecovery(_generation: number, error?: string) {
          onRecoveryComplete?.(error);
        },
      },
    } as Window,
  });
}

function sessionResult(content: string, revision: number, accepted = true, savedContent = "before") {
  return { accepted, content, path: "document.txt", revision, savedContent };
}

function mockModel(value: string): MockModel {
  return {
    disposed: false,
    value,
    version: 1,
    getValue() {
      return this.value;
    },
    getVersionId() {
      return this.version;
    },
    isDisposed() {
      return this.disposed;
    },
    dispose() {
      this.disposed = true;
    },
    setValue(next) {
      this.value = next;
      this.version += 1;
    },
  };
}
