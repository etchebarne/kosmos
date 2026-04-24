import { useCallback, useEffect, useRef, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { editor } from "monaco-editor";
import type { Workspace } from "../../../store/workspace.store";

// Monaco EditorOption numeric ids: fontSize = 61, lineHeight = 75.
const FONT_SIZE_OPT = 61;
const LINE_HEIGHT_OPT = 75;

/**
 * Shows an inline git-blame widget after the end of the cursor's current line.
 * The widget is debounced (500ms) and cleared on cursor movement or on unmount.
 */
export function useGitBlame(opts: {
  editorRef: RefObject<editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  workspace: Workspace | null;
  filePath: string | undefined;
  editorFontSize: number;
}): {
  clearBlameWidget: () => void;
  scheduleBlameUpdate: (line: number) => void;
} {
  const { editorRef, editorReady, workspace, filePath, editorFontSize } = opts;

  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const filePathRef = useRef(filePath);
  filePathRef.current = filePath;

  const widgetRef = useRef<editor.IContentWidget | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lineRef = useRef<number>(0);

  const clearBlameWidget = useCallback(() => {
    const ed = editorRef.current;
    if (ed && widgetRef.current) {
      ed.removeContentWidget(widgetRef.current);
      widgetRef.current = null;
    }
  }, [editorRef]);

  const updateBlame = useCallback(
    async (lineNumber: number) => {
      const ed = editorRef.current;
      const ws = workspaceRef.current;
      const fp = filePathRef.current;
      if (!ed || !ws || !fp) return;

      if (lineNumber === lineRef.current) return;
      lineRef.current = lineNumber;

      const relative = fp.startsWith(ws.path + "/") ? fp.slice(ws.path.length + 1) : fp;

      try {
        const blame = await invoke<string | null>("git_blame_line", {
          path: ws.path,
          file: relative,
          line: lineNumber,
        });

        // Cursor may have moved during the await; bail if so.
        if (lineRef.current !== lineNumber) return;

        clearBlameWidget();
        if (blame) {
          const model = ed.getModel();
          const endCol = model ? model.getLineMaxColumn(lineNumber) : 1;
          const domNode = document.createElement("div");
          domNode.className = "git-blame-inline";
          domNode.style.fontSize = `${ed.getOption(FONT_SIZE_OPT) * 0.85}px`;
          domNode.style.lineHeight = `${ed.getOption(LINE_HEIGHT_OPT)}px`;
          domNode.textContent = blame;
          const widget: editor.IContentWidget = {
            getId: () => "git-blame-widget",
            getDomNode: () => domNode,
            getPosition: () => ({
              position: { lineNumber, column: endCol },
              preference: [0],
            }),
          };
          widgetRef.current = widget;
          ed.addContentWidget(widget);
        }
      } catch {
        clearBlameWidget();
      }
    },
    [editorRef, clearBlameWidget],
  );

  const scheduleBlameUpdate = useCallback(
    (line: number) => {
      if (timerRef.current != null) clearTimeout(timerRef.current);
      clearBlameWidget();
      lineRef.current = 0;
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        updateBlame(line);
      }, 500);
    },
    [clearBlameWidget, updateBlame],
  );

  // Initial blame render once editor + workspace are ready.
  useEffect(() => {
    if (!editorReady || !workspace) return;
    const pos = editorRef.current?.getPosition();
    if (pos) updateBlame(pos.lineNumber);
  }, [editorReady, workspace, editorRef, updateBlame]);

  // Restyle the widget when editor font size changes.
  useEffect(() => {
    const widget = widgetRef.current;
    if (!widget) return;
    const ed = editorRef.current;
    const fs = ed?.getOption(FONT_SIZE_OPT);
    const lh = ed?.getOption(LINE_HEIGHT_OPT);
    if (fs) widget.getDomNode().style.fontSize = `${fs * 0.85}px`;
    if (lh) widget.getDomNode().style.lineHeight = `${lh}px`;
  }, [editorFontSize, editorRef]);

  // Cleanup on unmount.
  useEffect(() => {
    return () => {
      if (timerRef.current != null) clearTimeout(timerRef.current);
      clearBlameWidget();
    };
  }, [clearBlameWidget]);

  return { clearBlameWidget, scheduleBlameUpdate };
}
