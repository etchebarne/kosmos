import { describe, it, expect } from "vitest";
import { buildChangeTree } from "../gitTree";
import type { GitFileChange } from "../gitTree";

function change(path: string, status = "modified"): GitFileChange {
  return { path, status, staged: false, additions: 0, deletions: 0 };
}

describe("buildChangeTree", () => {
  it("returns empty array for empty input", () => {
    expect(buildChangeTree([])).toEqual([]);
  });

  it("handles flat files", () => {
    const result = buildChangeTree([change("a.ts"), change("b.ts")]);
    expect(result).toHaveLength(2);
    expect(result[0].name).toBe("a.ts");
    expect(result[1].name).toBe("b.ts");
  });

  it("groups files into directories", () => {
    const result = buildChangeTree([change("src/a.ts"), change("src/b.ts")]);
    expect(result).toHaveLength(1);
    expect(result[0].isDir).toBe(true);
    expect(result[0].name).toBe("src");
    expect(result[0].children).toHaveLength(2);
  });

  it("collapses single-child directories", () => {
    const result = buildChangeTree([change("src/lib/util.ts")]);
    expect(result).toHaveLength(1);
    expect(result[0].isDir).toBe(true);
    expect(result[0].name).toBe("src/lib");
    expect(result[0].children).toHaveLength(1);
    expect(result[0].children[0].name).toBe("util.ts");
  });

  it("sorts dirs before files", () => {
    const result = buildChangeTree([change("z.ts"), change("a/file.ts")]);
    expect(result[0].isDir).toBe(true);
    expect(result[1].isDir).toBe(false);
  });
});
