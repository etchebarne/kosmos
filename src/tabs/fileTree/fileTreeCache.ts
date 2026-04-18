import { invoke } from "@tauri-apps/api/core";
import type { DirEntry } from "./fileTreeTypes";

const cache = new Map<string, DirEntry[]>();
const inflight = new Map<string, Promise<DirEntry[]>>();

/**
 * Start loading a directory's entries into the cache.
 * No-op if already cached or in-flight.
 */
export function prefetch(dirPath: string): void {
  if (cache.has(dirPath) || inflight.has(dirPath)) return;
  fetchAndCache(dirPath);
}

/**
 * Return cached entries for a directory, or null if not yet available.
 */
export function getCached(dirPath: string): DirEntry[] | null {
  return cache.get(dirPath) ?? null;
}

/**
 * Return entries from cache or fetch them. Deduplicates in-flight requests.
 */
export function getOrFetch(dirPath: string): Promise<DirEntry[]> {
  const cached = cache.get(dirPath);
  if (cached) return Promise.resolve(cached);
  return inflight.get(dirPath) ?? fetchAndCache(dirPath);
}

/**
 * Remove a directory from the cache so the next read re-fetches from disk.
 */
export function invalidate(dirPath: string): void {
  cache.delete(dirPath);
}

function fetchAndCache(dirPath: string): Promise<DirEntry[]> {
  const promise = invoke<DirEntry[]>("read_dir", { path: dirPath }).then(
    (entries) => {
      cache.set(dirPath, entries);
      inflight.delete(dirPath);
      return entries;
    },
    (err) => {
      inflight.delete(dirPath);
      throw err;
    },
  );
  inflight.set(dirPath, promise);
  return promise;
}
