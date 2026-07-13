import type { KosmosIpcError, KosmosIpcRequestResult } from "../../shared/ipc";
import { errorMessage } from "../error-message";
import { KosmosIpcRequestError } from "../server/client";

export function ipcRequestFailure(error: unknown): KosmosIpcRequestResult<never> {
  return { ok: false, error: ipcRequestError(error) };
}

function ipcRequestError(error: unknown): KosmosIpcError {
  if (error instanceof KosmosIpcRequestError) {
    return { code: error.code, message: error.messageWithoutCode };
  }

  return { code: "ipc.request_failed", message: errorMessage(error) };
}
