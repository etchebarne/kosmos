import { afterEach, describe, expect, test } from "bun:test";

import {
  canConsumeRequest,
  createRequestGeneration,
  isCurrentRequest,
  matchesCurrentQuery,
} from "@/renderer/lib/request-generation";
import { useWorkspaceStore } from "@/renderer/stores/workspace-store";
import type {
  KosmosApi,
  KosmosIpcRequest,
  KosmosIpcRequestResult,
  OpenEditorLocationPayload,
} from "@/shared/ipc";

const originalWindow = globalThis.window;

afterEach(() => {
  useWorkspaceStore.setState({ error: null, pendingEditorSelection: null, snapshot: null });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: originalWindow,
  });
});

describe("request generations", () => {
  test("only the latest navigation request may apply state", () => {
    expect(isCurrentRequest(4, 4)).toBe(true);
    expect(isCurrentRequest(3, 4)).toBe(false);
    expect(canConsumeRequest(4, 4)).toBe(true);
    expect(canConsumeRequest(null, 4)).toBe(false);
    expect(canConsumeRequest(3, 4)).toBe(false);
  });

  test("workspace symbol results must match both query and generation", () => {
    const result = { generation: 7, query: "main" };
    expect(matchesCurrentQuery(result, 7, "main")).toBe(true);
    expect(matchesCurrentQuery(result, 8, "main")).toBe(false);
    expect(matchesCurrentQuery(result, 7, "map")).toBe(false);
  });

  test("manual invalidation makes queued navigation stale", () => {
    const requests = createRequestGeneration();
    const queuedNavigation = requests.issue();
    requests.invalidate();
    expect(requests.isCurrent(queuedNavigation)).toBe(false);
  });

  test("a current navigation applies one atomic location response", async () => {
    const requests: KosmosIpcRequest[] = [];
    installApi(async (request) => {
      requests.push(request);
      return successfulResult(locationResult(2, 9, "src/main.rs"));
    });

    const opened = await useWorkspaceStore
      .getState()
      .openEditorLocation(2, "src/main.rs", 4, 2);

    expect(opened).toBe(true);
    expect(requests).toEqual([
      {
        domain: "editor",
        action: "openLocation",
        params: { workspaceId: 2, path: "src/main.rs" },
      },
    ]);
    expect(useWorkspaceStore.getState().snapshot).toEqual(locationResult(2, 9, "src/main.rs").snapshot);
    expect(useWorkspaceStore.getState().pendingEditorSelection).toMatchObject({
      tabId: 9,
      workspaceId: 2,
      path: "src/main.rs",
    });
  });

  test("a stale successful navigation cannot apply its response", async () => {
    const pending = pendingRequests();
    installApi(pending.request);

    const first = useWorkspaceStore.getState().openEditorLocation(1, "first.rs", 1, 1);
    await waitForRequests(pending.requests, 1);
    const second = useWorkspaceStore.getState().openEditorLocation(2, "second.rs", 1, 1);
    pending.requests[0]?.resolve(successfulResult(locationResult(1, 4, "first.rs")));
    await waitForRequests(pending.requests, 2);
    pending.requests[1]?.resolve(successfulResult(locationResult(2, 5, "second.rs")));

    expect(await first).toBe(false);
    expect(await second).toBe(true);
    expect(pending.requests.map((request) => request.message.action)).toEqual([
      "openLocation",
      "openLocation",
    ]);
    expect(useWorkspaceStore.getState().snapshot).toEqual(locationResult(2, 5, "second.rs").snapshot);
  });

  test("a stale failed navigation cannot clear the current selection", async () => {
    const pending = pendingRequests();
    installApi(pending.request);

    const first = useWorkspaceStore.getState().openEditorLocation(1, "missing.rs", 1, 1);
    await waitForRequests(pending.requests, 1);
    const second = useWorkspaceStore.getState().openEditorLocation(2, "second.rs", 1, 1);
    pending.requests[0]?.resolve(failedResult("editor.file_not_found", "file is missing"));
    await waitForRequests(pending.requests, 2);
    pending.requests[1]?.resolve(successfulResult(locationResult(2, 5, "second.rs")));

    expect(await first).toBe(false);
    expect(await second).toBe(true);
    expect(useWorkspaceStore.getState().error).toBeNull();
    expect(useWorkspaceStore.getState().pendingEditorSelection).toMatchObject({
      tabId: 5,
      workspaceId: 2,
      path: "second.rs",
    });
  });
});

type PendingRequest = {
  message: KosmosIpcRequest;
  resolve(result: KosmosIpcRequestResult<OpenEditorLocationPayload>): void;
};

function pendingRequests() {
  const requests: PendingRequest[] = [];
  return {
    requests,
    request(message: KosmosIpcRequest): Promise<KosmosIpcRequestResult<OpenEditorLocationPayload>> {
      return new Promise((resolve) => {
        requests.push({ message, resolve });
      });
    },
  };
}

function installApi(
  request: (request: KosmosIpcRequest) => Promise<KosmosIpcRequestResult<OpenEditorLocationPayload>>,
): void {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      kosmos: {
        request,
      } as KosmosApi,
    } as Window,
  });
}

function locationResult(
  workspaceId: number,
  tabId: number,
  path: string,
): OpenEditorLocationPayload {
  return {
    snapshot: { activeWorkspaceId: workspaceId, workspaces: [] },
    target: { workspaceId, tabId, path },
  };
}

function successfulResult(
  result: OpenEditorLocationPayload,
): KosmosIpcRequestResult<OpenEditorLocationPayload> {
  return { ok: true, result };
}

function failedResult(code: string, message: string): KosmosIpcRequestResult<OpenEditorLocationPayload> {
  return { ok: false, error: { code, message } };
}

async function waitForRequests(requests: PendingRequest[], count: number): Promise<void> {
  for (let attempts = 0; attempts < 10 && requests.length < count; attempts += 1) {
    await Promise.resolve();
  }
  expect(requests).toHaveLength(count);
}
