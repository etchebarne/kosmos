import type { KosmosIpcRequestResult } from "../shared/ipc";

export function reconstructIpcRequestResult<T>(response: unknown): KosmosIpcRequestResult<T> {
  if (!response || typeof response !== "object" || !("ok" in response)) {
    return invalidResponse();
  }

  if (response.ok === true && "result" in response) {
    return { ok: true, result: response.result as T };
  }

  if (
    response.ok === false &&
    "error" in response &&
    response.error &&
    typeof response.error === "object" &&
    "code" in response.error &&
    typeof response.error.code === "string" &&
    "message" in response.error &&
    typeof response.error.message === "string"
  ) {
    return {
      ok: false,
      error: { code: response.error.code, message: response.error.message },
    };
  }

  return invalidResponse();
}

function invalidResponse(): KosmosIpcRequestResult<never> {
  return {
    ok: false,
    error: {
      code: "ipc.invalid_response",
      message: "Kosmos returned an invalid IPC response.",
    },
  };
}
