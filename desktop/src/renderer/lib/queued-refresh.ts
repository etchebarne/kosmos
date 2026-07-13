export function createQueuedRefresh(refresh: () => Promise<void>): () => Promise<void> {
  let requested = false;
  let active: Promise<void> | null = null;

  return () => {
    requested = true;
    active ??= (async () => {
      do {
        requested = false;
        await refresh();
      } while (requested);
    })().finally(() => {
      active = null;
    });
    return active;
  };
}
