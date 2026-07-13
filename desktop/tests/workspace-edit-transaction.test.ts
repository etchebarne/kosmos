import { describe, expect, test } from "bun:test";

import { replaceWorkspaceEditModel } from "../src/renderer/lib/workspace-edit-transaction";

describe("workspace edit Monaco adapter", () => {
  test("applies an exact core-provided replacement and clears local undo", () => {
    let value = "before";
    const ordinaryUndo = ["user edit"];
    const model = {
      getValue: () => value,
      setValue(next: string) {
        value = next;
        ordinaryUndo.length = 0;
      },
    };

    replaceWorkspaceEditModel(model, "after");

    expect(value).toBe("after");
    expect(ordinaryUndo).toEqual([]);
  });

  test("rejects a Monaco model that cannot apply the core replacement", () => {
    const model = {
      getValue: () => "before",
      setValue(_next: string) {},
    };

    expect(() => replaceWorkspaceEditModel(model, "after")).toThrow("rejected");
  });
});
