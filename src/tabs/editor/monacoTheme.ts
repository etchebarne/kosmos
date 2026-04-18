import type { Monaco } from "@monaco-editor/react";
import { getTheme } from "../../lib/themes";

export function defineKosmosTheme(monaco: Monaco) {
  const t = getTheme();
  monaco.editor.defineTheme("kosmos", {
    base: t.type === "dark" ? "vs-dark" : "vs",
    inherit: true,
    rules: [{ token: "tag", foreground: "569cd6" }],
    colors: {
      "editor.background": t.editor.background,
      "editor.foreground": t.editor.foreground,
      "editor.lineHighlightBackground": t.editor.lineHighlight,
      "editor.selectionBackground": t.editor.selection,
      "editor.inactiveSelectionBackground": t.editor.inactiveSelection,
      "editorLineNumber.foreground": t.editor.lineNumber,
      "editorLineNumber.activeForeground": t.editor.lineNumberActive,
      "editorCursor.foreground": t.editor.cursor,
      "editorIndentGuide.background": t.editor.indentGuide,
      "editorIndentGuide.activeBackground": t.editor.indentGuideActive,
      "editorWidget.background": t.editor.widget,
      "editorWidget.border": t.editor.widgetBorder,
      "editorSuggestWidget.background": t.editor.suggestBackground,
      "editorSuggestWidget.border": t.editor.suggestBorder,
      "editorSuggestWidget.selectedBackground": t.editor.suggestSelected,
      "scrollbarSlider.background": t.ui.scrollbar.track,
      "scrollbarSlider.hoverBackground": t.ui.scrollbar.hover,
      "scrollbarSlider.activeBackground": t.ui.scrollbar.active,
    },
  });
}
