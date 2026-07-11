import type { editor } from "monaco-editor";

import { attachLanguageDocument, type LanguageDocumentHandle } from "./language-client";

export type EditorBuffer = {
  model: editor.ITextModel;
  path: string;
  savedContent: string;
  languageDocument: LanguageDocumentHandle;
};

const buffers = new Map<string, EditorBuffer>();

export function getOrCreateEditorBuffer(
  workspaceId: number,
  tabId: number,
  path: string,
  content: string,
  createModel: () => editor.ITextModel,
): EditorBuffer {
  const key = bufferKey(workspaceId, tabId);
  const existing = buffers.get(key);

  if (existing?.path === path && !existing.model.isDisposed()) {
    return existing;
  }

  existing?.languageDocument.dispose();
  existing?.model.dispose();

  const model = createModel();
  const buffer = {
    model,
    path,
    savedContent: content,
    languageDocument: attachLanguageDocument(workspaceId, tabId, path, model),
  };
  buffers.set(key, buffer);

  return buffer;
}

export function reconcileEditorBuffer(buffer: EditorBuffer, content: string): boolean {
  const wasDirty = buffer.model.getValue() !== buffer.savedContent;
  buffer.savedContent = content;

  if (!wasDirty && buffer.model.getValue() !== content) {
    buffer.model.setValue(content);
  }

  return buffer.model.getValue() !== buffer.savedContent;
}

export function disposeEditorBuffer(workspaceId: number, tabId: number): void {
  const key = bufferKey(workspaceId, tabId);
  const buffer = buffers.get(key);

  buffer?.languageDocument.dispose();
  buffer?.model.dispose();
  buffers.delete(key);
}

export function disposeWorkspaceEditorBuffers(workspaceId: number): void {
  const prefix = `${workspaceId}:`;

  for (const [key, buffer] of buffers) {
    if (!key.startsWith(prefix)) {
      continue;
    }

    buffer.languageDocument.dispose();
    buffer.model.dispose();
    buffers.delete(key);
  }
}

function bufferKey(workspaceId: number, tabId: number): string {
  return `${workspaceId}:${tabId}`;
}
