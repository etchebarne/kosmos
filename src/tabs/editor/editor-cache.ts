import type { editor } from "monaco-editor";

export interface EditorCacheEntry {
  instance: editor.IStandaloneCodeEditor;
  pendingReveal?: { lineNumber: number; column: number };
  /** Save this editor's buffer to disk. Registered by EditorTab on mount. */
  save?: () => Promise<void> | void;
}

export const editorCache = new Map<string, EditorCacheEntry>();
export const editorOpenerRegistered = { value: false };

export function cleanupEditorInstances(workspacePath: string) {
  for (const key of editorCache.keys()) {
    if (key.startsWith(workspacePath)) editorCache.delete(key);
  }
}

export function revealPosition(filePath: string, position: { lineNumber: number; column: number }) {
  const entry = editorCache.get(filePath);
  if (entry) {
    entry.pendingReveal = position;
  } else {
    editorCache.set(filePath, { instance: null!, pendingReveal: position });
  }

  setTimeout(() => {
    const cached = editorCache.get(filePath);
    if (!cached || cached.pendingReveal !== position) return;
    if (cached.instance) {
      cached.pendingReveal = undefined;
      cached.instance.setPosition(position);
      cached.instance.revealPositionInCenter(position);
    }
  }, 50);
}
