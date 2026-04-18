import type { editor } from "monaco-editor";

/**
 * Base Monaco editor options shared across the main editor and read-only previews.
 * Consumers can spread this and override individual properties as needed.
 */
export const BASE_EDITOR_OPTIONS: editor.IStandaloneEditorConstructionOptions = {
  fontFamily: "'JetBrains Mono', monospace",
  lineHeight: 1.6,
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  padding: { top: 12 },
  smoothScrolling: true,
  overviewRulerBorder: false,
  hideCursorInOverviewRuler: true,
  scrollbar: {
    verticalScrollbarSize: 6,
    horizontalScrollbarSize: 6,
    useShadows: false,
  },
  wordWrap: "on",
  roundedSelection: false,
  contextmenu: false,
  automaticLayout: true,
  fixedOverflowWidgets: true,
};
