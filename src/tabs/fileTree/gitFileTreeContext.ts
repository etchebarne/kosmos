import { createContext } from "react";
import type { GitFileChange } from "../../lib/gitTree";
import { normalizePath } from "../../lib/pathUtils";

type GitColorFn = (entryPath: string, isDir: boolean) => string | null;

export const GitFileTreeContext = createContext<GitColorFn>(() => null);

const STATUS_PRIORITY: Record<string, number> = {
  deleted: 3,
  modified: 2,
  renamed: 2,
  added: 1,
  untracked: 1,
};

const PRIORITY_COLOR: Record<number, string> = {
  3: "text-[var(--color-status-red)]",
  2: "text-[var(--color-status-amber)]",
  1: "text-[var(--color-status-green)]",
};

export function buildGitColorLookup(changes: GitFileChange[], workspacePath: string): GitColorFn {
  if (changes.length === 0) return () => null;

  const fileColorMap = new Map<string, string>();
  const dirPriorityMap = new Map<string, number>();

  for (const change of changes) {
    const norm = normalizePath(change.path);
    const priority = STATUS_PRIORITY[change.status] ?? 1;
    fileColorMap.set(norm, PRIORITY_COLOR[priority]!);

    // Propagate to all ancestor directories
    const parts = norm.split("/");
    for (let i = 1; i < parts.length; i++) {
      const dir = parts.slice(0, i).join("/");
      const cur = dirPriorityMap.get(dir) ?? 0;
      if (priority > cur) dirPriorityMap.set(dir, priority);
    }
  }

  const wsPrefix = normalizePath(workspacePath).replace(/\/$/, "") + "/";

  return (entryPath: string, isDir: boolean) => {
    const norm = normalizePath(entryPath);
    if (!norm.startsWith(wsPrefix)) return null;
    const rel = norm.slice(wsPrefix.length);
    if (!rel) return null;

    if (isDir) {
      const p = dirPriorityMap.get(rel);
      return p ? (PRIORITY_COLOR[p] ?? null) : null;
    }
    return fileColorMap.get(rel) ?? null;
  };
}
