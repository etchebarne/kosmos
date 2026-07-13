import { describe, expect, test } from "bun:test";

import { ipcRequestFailure } from "../src/main/ipc/request-result";
import { KosmosIpcRequestError as MainIpcRequestError } from "../src/main/server/client";
import { reconstructIpcRequestResult } from "../src/preload/request-result";
import {
  KosmosIpcRequestError,
  requestResultValue,
} from "../src/renderer/lib/errors";

describe("IPC request errors", () => {
  test("preserves a typed workspace-trust error through main, preload, and renderer", () => {
    const mainResult = ipcRequestFailure(
      new MainIpcRequestError(
        "language_servers.workspace_not_trusted",
        "workspace trust is required",
      ),
    );
    const preloadResult = reconstructIpcRequestResult(mainResult);

    expect(mainResult).toEqual({
      ok: false,
      error: {
        code: "language_servers.workspace_not_trusted",
        message: "workspace trust is required",
      },
    });
    expect(preloadResult).toEqual(mainResult);

    try {
      requestResultValue(preloadResult);
      throw new Error("expected the IPC result to throw");
    } catch (caughtError) {
      expect(caughtError).toBeInstanceOf(KosmosIpcRequestError);
      expect((caughtError as KosmosIpcRequestError).code).toBe(
        "language_servers.workspace_not_trusted",
      );
    }
  });
});
