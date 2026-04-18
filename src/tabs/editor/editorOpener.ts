import type { Monaco } from "@monaco-editor/react";
import type { editor, Uri, IRange, IPosition } from "monaco-editor";
import { fileUriToPath } from "../../lib/lsp/uri";
import { useLayoutStore } from "../../store/layout.store";
import { getFileName } from "../../lib/pathUtils";
import { editorOpenerRegistered, revealPosition } from "./editorCache";

function uriToNormalizedPath(uri: string): string {
  let p = fileUriToPath(uri);
  if (/^[a-z]:/.test(p)) {
    p = p[0].toUpperCase() + p.slice(1);
  }
  return p;
}

export function registerEditorOpener(monaco: Monaco) {
  if (editorOpenerRegistered.value) return;
  editorOpenerRegistered.value = true;

  monaco.editor.registerEditorOpener({
    openCodeEditor(
      source: editor.ICodeEditor,
      resource: Uri,
      selectionOrPosition?: IRange | IPosition,
    ) {
      const sourceModel = source.getModel();
      if (sourceModel && sourceModel.uri.toString() === resource.toString()) {
        return false;
      }

      const filePath = uriToNormalizedPath(resource.toString());
      const fileName = getFileName(filePath);

      let position: { lineNumber: number; column: number } | undefined;
      if (selectionOrPosition) {
        if ("lineNumber" in selectionOrPosition) {
          position = {
            lineNumber: selectionOrPosition.lineNumber,
            column: selectionOrPosition.column,
          };
        } else {
          position = {
            lineNumber: selectionOrPosition.startLineNumber,
            column: selectionOrPosition.startColumn,
          };
        }
      }

      const store = useLayoutStore.getState();
      store.openFile(filePath, fileName, store.activePaneId ?? "");

      if (position) {
        revealPosition(filePath, position);
      }

      return true;
    },
  });
}
