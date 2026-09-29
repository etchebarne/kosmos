import { afterEach, describe, expect, test } from "bun:test";

import {
  disposeEditorBuffer,
  flushEditorBuffer,
  getOrCreateEditorBuffer,
  initializeEditorBufferRecovery,
  isEditorBufferDirty,
  queueEditorBufferSynchronization,
  setLanguageDocumentAttacher,
} from "@/renderer/lib/editor-buffers";
import type { KosmosApi, KosmosIpcRequest } from "@/shared/ipc";

import { MockTextModel } from "./support/mock-text-model";

setLanguageDocumentAttacher(() => ({ dispose() {} }));
const originalWindow = globalThis.window;

afterEach(() => {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: originalWindow,
  });
});

describe("document lifecycle", () => {
  test("rapid edits reach the server as one ordered batch of incremental edits", async () => {
    const requests: KosmosIpcRequest[] = [];
    installApi((request) => {
      requests.push(request);
      return ack((request.params as { revision: number }).revision);
    });
    const model = new MockTextModel("hello");
    const buffer = getOrCreateEditorBuffer(901, 1, "document.txt", "hello", () => model.asModel());

    queueEditorBufferSynchronization(buffer, model.edit({ offset: 5, length: 0, text: " world" }));
    queueEditorBufferSynchronization(buffer, model.edit({ offset: 0, length: 1, text: "H" }));
    await flushEditorBuffer(buffer);

    expect(requests).toHaveLength(1);
    expect(requests[0]?.action).toBe("changeSession");
    expect(requests[0]?.params).toMatchObject({
      baseRevision: 0,
      edits: [
        { offset: 5, length: 0, text: " world" },
        { offset: 0, length: 1, text: "H" },
      ],
    });
    expect(buffer.session.syncedRevision).toBe(buffer.session.revision);
    disposeEditorBuffer(901, 1);
  });

  test("edits never send the whole document", async () => {
    const requests: KosmosIpcRequest[] = [];
    installApi((request) => {
      requests.push(request);
      return ack((request.params as { revision: number }).revision);
    });
    const text = "x".repeat(100_000);
    const model = new MockTextModel(text);
    const buffer = getOrCreateEditorBuffer(907, 1, "big.txt", text, () => model.asModel());

    queueEditorBufferSynchronization(buffer, model.edit({ offset: 50_000, length: 0, text: "y" }));
    await flushEditorBuffer(buffer);

    expect(JSON.stringify(requests[0]?.params).length).toBeLessThan(500);
    disposeEditorBuffer(907, 1);
  });

  test("a rejected batch resynchronizes the model's full text without marking it clean", async () => {
    const requests: KosmosIpcRequest[] = [];
    installApi((request) => {
      requests.push(request);
      return request.action === "changeSession"
        ? ack(4, false)
        : ack((request.params as { revision: number }).revision);
    });
    const model = new MockTextModel("before");
    const buffer = getOrCreateEditorBuffer(902, 1, "document.txt", "before", () => model.asModel());

    queueEditorBufferSynchronization(buffer, model.setValue("newer local text"));
    await flushEditorBuffer(buffer);

    expect(requests.map((request) => request.action)).toEqual(["changeSession", "openSession"]);
    expect(requests[1]?.params).toMatchObject({ content: "newer local text" });
    expect((requests[1]?.params as { revision: number }).revision).toBeGreaterThan(4);
    expect(buffer.savedContent).toBe("before");
    expect(isEditorBufferDirty(buffer)).toBe(true);
    disposeEditorBuffer(902, 1);
  });

  test("typing queues IPC without awaiting it", () => {
    installApi(() => new Promise(() => {}));
    const model = new MockTextModel("before");
    const buffer = getOrCreateEditorBuffer(903, 1, "document.txt", "before", () => model.asModel());

    queueEditorBufferSynchronization(buffer, model.setValue("typed immediately"));

    expect(buffer.model.getValue()).toBe("typed immediately");
    expect(buffer.session.synchronization).not.toBeNull();
    disposeEditorBuffer(903, 1);
  });

  test("dirty state follows the model version without comparing text", () => {
    installApi(() => new Promise(() => {}));
    const model = new MockTextModel("saved");
    const buffer = getOrCreateEditorBuffer(908, 1, "document.txt", "saved", () => model.asModel());

    expect(isEditorBufferDirty(buffer)).toBe(false);
    queueEditorBufferSynchronization(buffer, model.edit({ offset: 0, length: 0, text: "!" }));
    expect(isEditorBufferDirty(buffer)).toBe(true);
    disposeEditorBuffer(908, 1);
  });

  test("sidecar recovery preserves current text and the saved baseline", async () => {
    const requests: KosmosIpcRequest[] = [];
    let reconnect: ((generation: number) => void) | undefined;
    installApi(
      (request) => {
        requests.push(request);
        return ack((request.params as { revision: number }).revision);
      },
      (listener) => {
        reconnect = listener;
      },
    );
    initializeEditorBufferRecovery();
    const model = new MockTextModel("saved");
    const buffer = getOrCreateEditorBuffer(904, 1, "document.txt", "saved", () => model.asModel());
    model.setValue("unsaved");

    reconnect?.(1);
    await flushEditorBuffer(buffer);

    expect(requests).toHaveLength(1);
    expect(requests[0]?.action).toBe("restoreSession");
    expect(requests[0]?.params).toMatchObject({ content: "unsaved", savedContent: "saved" });
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
        const revision = (request.params as { revision: number }).revision;
        return requests.length === 1 ? ack(revision + 2, false) : ack(revision);
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
    const model = new MockTextModel("saved");
    const buffer = getOrCreateEditorBuffer(905, 1, "document.txt", "saved", () => model.asModel());
    model.setValue("local text");

    reconnect?.(1);
    await recoveredPromise;

    expect(requests.map((request) => request.action)).toEqual(["restoreSession", "openSession"]);
    expect((requests[1]?.params as { revision: number }).revision).toBeGreaterThan(
      (requests[0]?.params as { revision: number }).revision + 2,
    );
    expect(requests[1]?.params).toMatchObject({ content: "local text" });
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
        const revision = (request.params as { revision: number }).revision;
        if (requests.length === 1) {
          return new Promise((resolve) => {
            resolveFirst = () => resolve(ack(revision));
          });
        }
        return ack(revision);
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
    const model = new MockTextModel("saved");
    const buffer = getOrCreateEditorBuffer(906, 1, "document.txt", "saved", () => model.asModel());
    model.setValue("first edit");

    reconnect?.(1);
    while (!resolveFirst) {
      await Promise.resolve();
    }
    queueEditorBufferSynchronization(buffer, model.setValue("edit during recovery"));
    resolveFirst?.();
    await recoveredPromise;
    await flushEditorBuffer(buffer);

    expect(requests.map((request) => request.action)).toEqual([
      "restoreSession",
      "changeSession",
    ]);
    expect(requests[1]?.params).toMatchObject({
      edits: [{ offset: 0, length: "first edit".length, text: "edit during recovery" }],
    });
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

function ack(revision: number, accepted = true) {
  return { accepted, revision, savedGeneration: 0 };
}
