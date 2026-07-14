import { expect, test } from "bun:test";
import type { FileTree as FileTreeModel } from "@pierre/trees";

import { reconcileFileTreePaths } from "@/renderer/lib/file-tree-refresh";

type RefreshableFileTree = Pick<FileTreeModel, "getItem" | "resetPaths">;

test("file-tree refresh preserves local expansion for surviving directories", () => {
  const expandedPaths = new Set(["workspace/", "workspace/src/"]);
  const resetCalls: Array<{ paths: readonly string[]; expandedPaths: readonly string[] }> = [];
  const model = {
    getItem(path: string) {
      if (path === "workspace/new/") return null;
      return {
        isDirectory: () => path.endsWith("/"),
        isExpanded: () => expandedPaths.has(path),
      };
    },
    resetPaths(paths: readonly string[], options?: { initialExpandedPaths?: readonly string[] }) {
      resetCalls.push({ paths, expandedPaths: options?.initialExpandedPaths ?? [] });
    },
  } as unknown as RefreshableFileTree;

  const nextPaths = [
    "workspace/",
    "workspace/src/",
    "workspace/src/main.ts",
    "workspace/new/",
  ];

  expect(
    reconcileFileTreePaths(
      model,
      ["workspace/", "workspace/src/", "workspace/src/main.ts"],
      nextPaths,
    ),
  ).toBe(true);
  expect(resetCalls).toEqual([
    {
      paths: nextPaths,
      expandedPaths: ["workspace/", "workspace/src/"],
    },
  ]);
});

test("file-tree refresh leaves the live model untouched when paths have not changed", () => {
  let resetCount = 0;
  const model = {
    getItem: () => null,
    resetPaths: () => {
      resetCount += 1;
    },
  } as unknown as RefreshableFileTree;
  const paths = ["workspace/", "workspace/src/"];

  expect(reconcileFileTreePaths(model, paths, [...paths])).toBe(false);
  expect(resetCount).toBe(0);
});
