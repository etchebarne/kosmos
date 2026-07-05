export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return "Unable to communicate with the Kosmos server.";
}
