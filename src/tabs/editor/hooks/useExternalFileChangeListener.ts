import { useEffect, type MutableRefObject, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { editor } from "monaco-editor";
import { useLspStore } from "../../../store/lsp.store";
import type { Workspace } from "../../../store/workspace.store";
import { normalizePath } from "../../../lib/pathUtils";

/**
 * Reload the editor from disk when the file changes externally — but only while
 * the editor is clean. Also fans the update out to the LSP server(s).
 */
export function useExternalFileChangeListener(opts: {
  filePath: string | undefined;
  editorRef: RefObject<editor.IStandaloneCodeEditor | null>;
  contentRef: MutableRefObject<string | null>;
  dirtyRef: RefObject<boolean>;
  isExternalUpdateRef: MutableRefObject<boolean>;
  savedVersionIdRef: MutableRefObject<number>;
  versionRef: MutableRefObject<number>;
  workspaceRef: RefObject<Workspace | null>;
  fileUriRef: RefObject<string | null>;
  lspLanguageRef: RefObject<string>;
  onContentReplaced: (newContent: string) => void;
  clearDirty: () => void;
}): void {
  const {
    filePath,
    editorRef,
    contentRef,
    dirtyRef,
    isExternalUpdateRef,
    savedVersionIdRef,
    versionRef,
    workspaceRef,
    fileUriRef,
    lspLanguageRef,
    onContentReplaced,
    clearDirty,
  } = opts;

  useEffect(() => {
    if (!filePath) return;

    const unlisten = listen<string[]>("file-content-changed", async (event) => {
      const changedFiles = event.payload;
      const norm = normalizePath(filePath);
      if (!changedFiles.some((f) => normalizePath(f) === norm)) return;

      if (dirtyRef.current) return;

      try {
        const newContent = await invoke<string>("read_file", { path: filePath });
        // Skip self-triggered saves.
        if (newContent === contentRef.current) return;

        contentRef.current = newContent;
        const ed = editorRef.current;
        if (ed) {
          isExternalUpdateRef.current = true;
          ed.setValue(newContent);
          isExternalUpdateRef.current = false;
          const model = ed.getModel();
          if (model) savedVersionIdRef.current = model.getAlternativeVersionId();
          clearDirty();
        } else {
          onContentReplaced(newContent);
        }

        const ws = workspaceRef.current;
        const uri = fileUriRef.current;
        if (ws && uri) {
          versionRef.current++;
          const state = useLspStore.getState();
          const client = state.getClient(ws.path, lspLanguageRef.current);
          client?.didChange(uri, versionRef.current, [{ text: newContent }]);
          for (const companion of state.getCompanionClients(ws.path, lspLanguageRef.current)) {
            companion.didChange(uri, versionRef.current, [{ text: newContent }]);
          }
        }
      } catch {
        // File may have been deleted.
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [
    filePath,
    editorRef,
    contentRef,
    dirtyRef,
    isExternalUpdateRef,
    savedVersionIdRef,
    versionRef,
    workspaceRef,
    fileUriRef,
    lspLanguageRef,
    onContentReplaced,
    clearDirty,
  ]);
}
