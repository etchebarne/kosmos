import { afterEach, describe, expect, test } from "bun:test";

import {
  applyEditorSaveProjection,
  disposeEditorBuffer,
  editorSaveWarningMessage,
  getOrCreateEditorBuffer,
  isEditorBufferDirty,
  setLanguageDocumentAttacher,
} from "@/renderer/lib/editor-buffers";

import { MockTextModel } from "./support/mock-text-model";

setLanguageDocumentAttacher(() => ({ dispose() {} }));

describe("document save projection", () => {
  afterEach(() => {
    disposeEditorBuffer(712, 1);
    disposeEditorBuffer(712, 2);
    disposeEditorBuffer(712, 3);
  });

  test("applies formatted core content when the saved revision is still current", () => {
    const model = new MockTextModel("const value=1");
    const buffer = getOrCreateEditorBuffer(712, 1, "document.ts", "", () => model.asModel());
    buffer.session.revision = 4;

    expect(
      applyEditorSaveProjection(buffer, {
        currentRevision: 4,
        savedContent: "const value = 1;\n",
        savedGeneration: 2,
        savedRevision: 4,
        warnings: [],
      }),
    ).toBe(true);
    expect(buffer.model.getValue()).toBe("const value = 1;\n");
    expect(buffer.savedContent).toBe("const value = 1;\n");
    expect(buffer.session.savedGeneration).toBe(2);
    expect(isEditorBufferDirty(buffer)).toBe(false);
  });

  test("an unformatted save keeps the model text and marks it clean", () => {
    const model = new MockTextModel("typed text");
    const buffer = getOrCreateEditorBuffer(712, 3, "document.ts", "before", () => model.asModel());
    buffer.session.revision = 6;
    expect(isEditorBufferDirty(buffer)).toBe(true);

    expect(
      applyEditorSaveProjection(buffer, {
        currentRevision: 6,
        savedContent: null,
        savedGeneration: 1,
        savedRevision: 6,
        warnings: [],
      }),
    ).toBe(true);
    expect(buffer.savedContent).toBe("typed text");
    expect(isEditorBufferDirty(buffer)).toBe(false);
  });

  test("suppresses a stale save response after immediate typing", () => {
    const model = new MockTextModel("newer local text");
    const buffer = getOrCreateEditorBuffer(712, 2, "document.ts", "before", () => model.asModel());
    buffer.session.revision = 5;

    expect(
      applyEditorSaveProjection(buffer, {
        currentRevision: 4,
        savedContent: "formatted older text",
        savedGeneration: 1,
        savedRevision: 4,
        warnings: [],
      }),
    ).toBe(false);
    expect(buffer.model.getValue()).toBe("newer local text");
    expect(buffer.savedContent).toBe("before");
  });

  test("renders formatter failures as non-fatal save warnings", () => {
    expect(
      editorSaveWarningMessage({
        code: "formatters.execution_failed",
        kind: "formatting",
        message: "formatter exited with status 1",
      }),
    ).toBe("Formatting failed: formatter exited with status 1");
  });
});
