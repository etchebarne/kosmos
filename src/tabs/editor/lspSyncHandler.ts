import type { MutableRefObject, RefObject } from "react";
import type { editor } from "monaco-editor";
import { TextDocumentSyncKind } from "vscode-languageserver-protocol";
import type { TextDocumentContentChangeEvent } from "vscode-languageserver-protocol";
import { useLayoutStore } from "../../store/layout.store";
import { useLspStore } from "../../store/lsp.store";
import type { Workspace } from "../../store/workspace.store";

const DIDCHANGE_DEBOUNCE_MS = 200;

/**
 * Build the onDidChangeModelContent listener that:
 *   - mirrors buffer state into `contentRef` and `savedVersionIdRef`
 *   - flips the tab's dirty flag when the buffer diverges from last-saved
 *   - debounces LSP didChange notifications (full vs incremental based on server caps)
 *   - pokes the AI gutter so it refreshes once the user stops typing
 */
export function createModelContentChangeHandler(opts: {
  instance: editor.IStandaloneCodeEditor;
  tabId: string;
  workspace: Workspace | null;
  fileUri: string | null;
  contentRef: MutableRefObject<string | null>;
  isExternalUpdateRef: RefObject<boolean>;
  savedVersionIdRef: RefObject<number>;
  versionRef: MutableRefObject<number>;
  pendingChangesRef: MutableRefObject<TextDocumentContentChangeEvent[]>;
  debounceTimerRef: MutableRefObject<ReturnType<typeof setTimeout> | null>;
  lspLanguageRef: RefObject<string>;
  onContentChanged: () => void;
}): (e: editor.IModelContentChangedEvent) => void {
  const {
    instance,
    tabId,
    workspace,
    fileUri,
    contentRef,
    isExternalUpdateRef,
    savedVersionIdRef,
    versionRef,
    pendingChangesRef,
    debounceTimerRef,
    lspLanguageRef,
    onContentChanged,
  } = opts;

  return (e) => {
    contentRef.current = instance.getValue();
    // External reload: don't flip dirty.
    if (isExternalUpdateRef.current) return;

    const m = instance.getModel();
    const vid = m?.getAlternativeVersionId() ?? 0;
    const shouldBeDirty = vid !== savedVersionIdRef.current;
    const store = useLayoutStore.getState();
    if (shouldBeDirty !== store.dirtyTabs.has(tabId)) {
      store.setTabDirty(tabId, shouldBeDirty);
    }

    if (!workspace || !fileUri) return;
    const lspState = useLspStore.getState();
    const client = lspState.getClient(workspace.path, lspLanguageRef.current);
    if (!client) return;

    versionRef.current++;

    const syncKind =
      typeof client.capabilities?.textDocumentSync === "object"
        ? client.capabilities.textDocumentSync.change
        : client.capabilities?.textDocumentSync;

    if (syncKind === TextDocumentSyncKind.Full) {
      pendingChangesRef.current = [{ text: instance.getValue() }];
    } else {
      const changes = e.changes.map((change) => ({
        range: {
          start: {
            line: change.range.startLineNumber - 1,
            character: change.range.startColumn - 1,
          },
          end: {
            line: change.range.endLineNumber - 1,
            character: change.range.endColumn - 1,
          },
        },
        rangeLength: change.rangeLength,
        text: change.text,
      }));
      pendingChangesRef.current.push(...changes);
    }

    if (debounceTimerRef.current != null) {
      clearTimeout(debounceTimerRef.current);
    }
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      if (pendingChangesRef.current.length === 0) return;
      const current = useLspStore.getState();
      const currentClient = current.getClient(workspace.path, lspLanguageRef.current);
      currentClient?.didChange(fileUri, versionRef.current, pendingChangesRef.current);
      for (const companion of current.getCompanionClients(workspace.path, lspLanguageRef.current)) {
        companion.didChange(fileUri, versionRef.current, pendingChangesRef.current);
      }
      pendingChangesRef.current = [];
    }, DIDCHANGE_DEBOUNCE_MS);

    onContentChanged();
  };
}
