import type { editor } from "monaco-editor";

export type EditorBuffer = {
  model: editor.ITextModel;
  path: string;
  savedContent: string;
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

  existing?.model.dispose();

  const buffer = {
    model: createModel(),
    path,
    savedContent: content,
  };
  buffers.set(key, buffer);

  return buffer;
}

export function disposeEditorBuffer(workspaceId: number, tabId: number): void {
  const key = bufferKey(workspaceId, tabId);
  const buffer = buffers.get(key);

  buffer?.model.dispose();
  buffers.delete(key);
}

export function disposeWorkspaceEditorBuffers(workspaceId: number): void {
  const prefix = `${workspaceId}:`;

  for (const [key, buffer] of buffers) {
    if (!key.startsWith(prefix)) {
      continue;
    }

    buffer.model.dispose();
    buffers.delete(key);
  }
}

function bufferKey(workspaceId: number, tabId: number): string {
  return `${workspaceId}:${tabId}`;
}
