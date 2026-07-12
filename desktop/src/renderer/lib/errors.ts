import type { KosmosIpcError, KosmosIpcRequestResult } from "@/shared/ipc";

export class KosmosIpcRequestError extends Error {
  constructor(
    readonly code: KosmosIpcError["code"],
    message: string,
  ) {
    super(message);
    this.name = "KosmosIpcRequestError";
  }
}

export function requestResultValue<T>(response: KosmosIpcRequestResult<T>): T {
  if (response.ok) {
    return response.result;
  }

  throw new KosmosIpcRequestError(response.error.code, response.error.message);
}

export function hasIpcErrorCode(error: unknown, code: string): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === code
  );
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return "Unable to communicate with the Kosmos server.";
}
