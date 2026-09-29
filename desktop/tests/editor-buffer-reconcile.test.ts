import { describe, expect, test } from "bun:test";

import {
  disposeEditorBuffer,
  getOrCreateEditorBuffer,
  reconcileEditorBuffer,
  setLanguageDocumentAttacher,
} from "@/renderer/lib/editor-buffers";
import type { EditorDocument } from "@/shared/ipc";

type MockModel = ReturnType<typeof mockModel>;

setLanguageDocumentAttacher(() => ({ dispose() {} }));

describe("editor buffer reconciliation", () => {
  test("a clean buffer follows a document changed on disk", () => {
    const model = mockModel("before");
    const buffer = getOrCreateEditorBuffer(801, 1, "a.txt", "before", () => model as never);

    const isDirty = reconcileEditorBuffer(buffer, document("external", "external", 4));

    expect(isDirty).toBe(false);
    expect(model.getValue()).toBe("external");
    expect(model.undoableEdits).toBe(1);
    expect(buffer.session.revision).toBe(4);
    disposeEditorBuffer(801, 1);
  });

  test("a dirty buffer keeps its text and stays dirty against the new disk baseline", () => {
    const model = mockModel("before");
    const buffer = getOrCreateEditorBuffer(802, 1, "a.txt", "before", () => model as never);
    model.setValue("unsaved");

    const isDirty = reconcileEditorBuffer(buffer, document("unsaved", "external", 2));

    expect(isDirty).toBe(true);
    expect(model.getValue()).toBe("unsaved");
    expect(buffer.savedContent).toBe("external");
    disposeEditorBuffer(802, 1);
  });

  test("a session document echoing unsaved text never marks the buffer clean", () => {
    const model = mockModel("before");
    const buffer = getOrCreateEditorBuffer(803, 1, "a.txt", "before", () => model as never);
    model.setValue("unsaved");

    expect(reconcileEditorBuffer(buffer, document("unsaved", "before", 2))).toBe(true);
    disposeEditorBuffer(803, 1);
  });

  test("a renamed buffer keeps unsynchronized edits in its new model", () => {
    const original = mockModel("before");
    const buffer = getOrCreateEditorBuffer(804, 1, "a.txt", "before", () => original as never);
    original.setValue("unsaved");
    const renamed = mockModel("before");

    const retargeted = getOrCreateEditorBuffer(804, 1, "b.txt", "before", () => renamed as never);

    expect(retargeted).not.toBe(buffer);
    expect(renamed.getValue()).toBe("unsaved");
    expect(retargeted.savedContent).toBe("before");
    expect(original.isDisposed()).toBe(true);
    disposeEditorBuffer(804, 1);
  });
});

function document(content: string, savedContent: string, revision: number): EditorDocument {
  return { path: "a.txt", content, savedContent, revision, accepted: true };
}

function mockModel(value: string) {
  return {
    disposed: false,
    undoableEdits: 0,
    value,
    version: 1,
    getValue() {
      return this.value;
    },
    getVersionId() {
      return this.version;
    },
    getFullModelRange() {
      return {};
    },
    isDisposed() {
      return this.disposed;
    },
    dispose() {
      this.disposed = true;
    },
    setValue(next: string) {
      this.value = next;
      this.version += 1;
    },
    pushEditOperations(_selections: unknown, edits: { text: string }[]) {
      this.undoableEdits += 1;
      (this as MockModel).setValue(edits[0]?.text ?? "");
      return null;
    },
  };
}
