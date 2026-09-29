import { describe, expect, test } from "bun:test";

import {
  disposeEditorBuffer,
  getOrCreateEditorBuffer,
  isEditorBufferDirty,
  reconcileEditorBuffer,
  setLanguageDocumentAttacher,
} from "@/renderer/lib/editor-buffers";
import type { EditorDocument } from "@/shared/ipc";

import { MockTextModel } from "./support/mock-text-model";

setLanguageDocumentAttacher(() => ({ dispose() {} }));
Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: { kosmos: { request: () => new Promise(() => {}) } },
});

describe("editor buffer reconciliation", () => {
  test("a clean buffer follows a newer server document without echoing it back", () => {
    const model = new MockTextModel("before");
    const buffer = getOrCreateEditorBuffer(801, 1, "a.txt", "before", () => model.asModel());

    const isDirty = reconcileEditorBuffer(buffer, document("external", null, 4));

    expect(isDirty).toBe(false);
    expect(model.getValue()).toBe("external");
    expect(buffer.session.syncedRevision).toBe(4);
    expect(buffer.session.pendingEdits.every((edit) => edit.text === "")).toBe(true);
    disposeEditorBuffer(801, 1);
  });

  test("a dirty buffer keeps its text and stays dirty against the new disk baseline", () => {
    const model = new MockTextModel("before");
    const buffer = getOrCreateEditorBuffer(802, 1, "a.txt", "before", () => model.asModel());
    model.setValue("unsaved");

    const isDirty = reconcileEditorBuffer(buffer, document("unsaved", "external", 0));

    expect(isDirty).toBe(true);
    expect(model.getValue()).toBe("unsaved");
    expect(buffer.savedContent).toBe("external");
    disposeEditorBuffer(802, 1);
  });

  test("a document equal to the model marks the buffer clean", () => {
    const model = new MockTextModel("before");
    const buffer = getOrCreateEditorBuffer(803, 1, "a.txt", "before", () => model.asModel());
    model.setValue("saved elsewhere");

    expect(reconcileEditorBuffer(buffer, document("saved elsewhere", null, 0))).toBe(false);
    expect(isEditorBufferDirty(buffer)).toBe(false);
    disposeEditorBuffer(803, 1);
  });

  test("a renamed buffer keeps unsynchronized edits in its new model", () => {
    const original = new MockTextModel("before");
    const buffer = getOrCreateEditorBuffer(804, 1, "a.txt", "before", () => original.asModel());
    original.setValue("unsaved");
    const renamed = new MockTextModel("before");

    const retargeted = getOrCreateEditorBuffer(804, 1, "b.txt", "before", () =>
      renamed.asModel(),
    );

    expect(retargeted).not.toBe(buffer);
    expect(renamed.getValue()).toBe("unsaved");
    expect(retargeted.savedContent).toBe("before");
    expect(isEditorBufferDirty(retargeted)).toBe(true);
    expect(original.isDisposed()).toBe(true);
    disposeEditorBuffer(804, 1);
  });
});

function document(
  content: string,
  savedContent: string | null,
  revision: number,
): EditorDocument {
  return { path: "a.txt", content, savedContent, revision, savedGeneration: 1 };
}
