import { expect, test } from "bun:test";

import { formattingErrorAfterContextChange } from "@/renderer/lib/formatting-error-state";

test("formatting errors clear when formatting is disabled or the document changes", () => {
  expect(
    formattingErrorAfterContextChange("failed", {
      formattingEnabled: true,
      documentChanged: false,
    }),
  ).toBe("failed");
  expect(
    formattingErrorAfterContextChange("failed", {
      formattingEnabled: false,
      documentChanged: false,
    }),
  ).toBeNull();
  expect(
    formattingErrorAfterContextChange("failed", {
      formattingEnabled: true,
      documentChanged: true,
    }),
  ).toBeNull();
});
