import { afterEach, describe, expect, test } from "bun:test";

import {
  applyEditorSaveProjection,
  disposeEditorBuffer,
  editorSaveWarningMessage,
  getOrCreateEditorBuffer,
  setLanguageDocumentAttacher,
} from "@/renderer/lib/editor-buffers";

setLanguageDocumentAttacher(() => ({ dispose() {} }));

describe("document save projection", () => {
  afterEach(() => {
    disposeEditorBuffer(712, 1);
    disposeEditorBuffer(712, 2);
  });

  test("applies formatted core content when the saved revision is still current", () => {
    const model = mockModel("const value=1");
    const buffer = getOrCreateEditorBuffer(712, 1, "document.ts", model.getValue(), () => model as never);
    buffer.session.revision = 4;

    expect(
      applyEditorSaveProjection(buffer, {
        currentRevision: 4,
        savedContent: "const value = 1;\n",
        savedRevision: 4,
        warnings: [],
      }),
    ).toBe(true);
    expect(buffer.model.getValue()).toBe("const value = 1;\n");
    expect(buffer.savedContent).toBe("const value = 1;\n");
  });

  test("suppresses a stale save response after immediate typing", () => {
    const model = mockModel("newer local text");
    const buffer = getOrCreateEditorBuffer(712, 2, "document.ts", "before", () => model as never);
    buffer.session.revision = 5;

    expect(
      applyEditorSaveProjection(buffer, {
        currentRevision: 4,
        savedContent: "formatted older text",
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

function mockModel(value: string) {
  return {
    disposed: false,
    value,
    getValue() {
      return this.value;
    },
    getVersionId() {
      return 1;
    },
    isDisposed() {
      return this.disposed;
    },
    dispose() {
      this.disposed = true;
    },
    setValue(next: string) {
      this.value = next;
    },
  };
}
