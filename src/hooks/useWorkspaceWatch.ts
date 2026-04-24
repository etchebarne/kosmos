import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

const refCounts = new Map<string, number>();
let watchedPath: string | null = null;
let lastRequestedPath: string | null = null;
let syncPromise: Promise<void> = Promise.resolve();

function getDesiredPath(): string | null {
  if (lastRequestedPath && (refCounts.get(lastRequestedPath) ?? 0) > 0) {
    return lastRequestedPath;
  }
  for (const [path, count] of refCounts) {
    if (count > 0) return path;
  }
  return null;
}

function scheduleSync() {
  syncPromise = syncPromise.then(async () => {
    const desiredPath = getDesiredPath();
    if (desiredPath === watchedPath) return;

    if (watchedPath) {
      const path = watchedPath;
      watchedPath = null;
      await invoke("unwatch_workspace", { path }).catch((error) =>
        console.warn("Failed to stop file watcher:", error),
      );
    }

    if (!desiredPath) return;

    await invoke("watch_workspace", { path: desiredPath }).catch((error) => {
      console.warn("Failed to start file watcher:", error);
    });
    watchedPath = desiredPath;
  });

  return syncPromise;
}

function acquireWorkspaceWatch(path: string) {
  lastRequestedPath = path;
  refCounts.set(path, (refCounts.get(path) ?? 0) + 1);
  void scheduleSync();
}

function releaseWorkspaceWatch(path: string) {
  const current = refCounts.get(path);
  if (!current) return;
  if (current === 1) refCounts.delete(path);
  else refCounts.set(path, current - 1);
  if (lastRequestedPath === path && !refCounts.has(path)) {
    lastRequestedPath = null;
  }
  void scheduleSync();
}

export function useWorkspaceWatch(path: string | null, active = true): void {
  useEffect(() => {
    if (!path || !active) return;
    acquireWorkspaceWatch(path);
    return () => {
      releaseWorkspaceWatch(path);
    };
  }, [path, active]);
}
