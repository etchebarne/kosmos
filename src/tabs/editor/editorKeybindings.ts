import type { Monaco } from "@monaco-editor/react";
import type { editor, IDisposable } from "monaco-editor";

export interface EditorKeyHandlers {
  save: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
}

/**
 * Register per-instance save and zoom shortcuts (Ctrl/Cmd + S/=/-/0).
 * Uses `onKeyDown` rather than `addCommand` so multiple editors don't overwrite
 * each other in Monaco's global keybinding registry.
 */
export function attachEditorKeybindings(
  instance: editor.IStandaloneCodeEditor,
  monaco: Monaco,
  handlers: EditorKeyHandlers,
): IDisposable {
  return instance.onKeyDown((e) => {
    const ctrl = e.ctrlKey || e.metaKey;
    if (!ctrl || e.shiftKey || e.altKey) return;
    if (e.keyCode === monaco.KeyCode.KeyS) {
      e.preventDefault();
      e.stopPropagation();
      handlers.save();
      return;
    }
    if (e.keyCode === monaco.KeyCode.Equal) {
      e.preventDefault();
      e.stopPropagation();
      handlers.zoomIn();
      return;
    }
    if (e.keyCode === monaco.KeyCode.Minus) {
      e.preventDefault();
      e.stopPropagation();
      handlers.zoomOut();
      return;
    }
    if (e.keyCode === monaco.KeyCode.Digit0) {
      e.preventDefault();
      e.stopPropagation();
      handlers.resetZoom();
      return;
    }
  });
}
