export async function startWithFatalHandler(
  start: () => Promise<void>,
  onFailure: (error: unknown) => void,
): Promise<void> {
  try {
    await start();
  } catch (error: unknown) {
    onFailure(error);
  }
}
