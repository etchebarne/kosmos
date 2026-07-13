import { expect, test } from "bun:test";

import { fileTreeGitStatus } from "@/renderer/lib/file-tree-git-status";

test("file-tree Git status displays unstaged changes before staged changes", () => {
  const decoration = {
    path: "workspace/document.txt",
    staged: "added" as const,
    unstaged: "modified" as const,
  };

  expect(fileTreeGitStatus(decoration)).toBe("modified");
  expect(decoration.staged).toBe("added");
  expect(decoration.unstaged).toBe("modified");
});
