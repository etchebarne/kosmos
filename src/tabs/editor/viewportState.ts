import type { editor } from "monaco-editor";

// Module-level map; survives pane re-renders and tab moves (keyed by tab id, which
// never changes once a tab is created). Cleared only when the tab is closed.
const viewStates = new Map<string, editor.ICodeEditorViewState>();

export function saveViewportState(tabId: string, state: editor.ICodeEditorViewState | null): void {
  if (state) viewStates.set(tabId, state);
}

export function getViewportState(tabId: string): editor.ICodeEditorViewState | undefined {
  return viewStates.get(tabId);
}

export function clearViewportState(tabId: string): void {
  viewStates.delete(tabId);
}
