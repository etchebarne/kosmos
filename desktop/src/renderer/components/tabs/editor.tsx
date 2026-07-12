import { useEffect, useRef, useState } from "react";

import { getEditorDocument, getEditorGitLineHunks, saveEditorDocument } from "@/renderer/ipc";
import {
  getOrCreateEditorBuffer,
  assertEditorBufferEditable,
  flushEditorBuffer,
  isEditorBufferLocked,
  openEditorBufferSession,
  queueEditorBufferSynchronization,
  reconcileEditorBuffer,
  subscribeEditorBufferLock,
  subscribeEditorBufferModel,
  type EditorBuffer,
} from "@/renderer/lib/editor-buffers";
import { editorSettings } from "@/renderer/lib/editor-settings";
import {
  formatLanguageDocument,
  notifyLanguageDocumentSaved,
} from "@/renderer/lib/language-client";
import { editorGitDecorations } from "@/renderer/lib/editor-git-decorations";
import { createDocumentSaveCoordinator } from "@/renderer/lib/document-save-coordinator";
import { errorMessage } from "@/renderer/lib/errors";
import { formattingErrorAfterContextChange } from "@/renderer/lib/formatting-error-state";
import { applyMonacoTheme, monaco } from "@/renderer/lib/monaco";
import { useGitStore, useSettingsStore, useWorkspaceStore } from "@/renderer/stores";
import type { EditorDocument, EditorGitLineHunk, TabId, WorkspaceId } from "@/shared/ipc";

type EditorTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  isActive: boolean;
  onActivatePane(): void;
};

type EditorLoadState =
  | { status: "loading"; workspaceId: WorkspaceId; tabId: TabId }
  | {
      status: "loaded";
      workspaceId: WorkspaceId;
      tabId: TabId;
      document: EditorDocument;
      gitLineHunks: EditorGitLineHunk[];
    }
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string };

type SaveState =
  | { status: "clean" }
  | { status: "dirty" }
  | { status: "saving" }
  | { status: "error"; message: string };

export function EditorTab({ workspaceId, tabId, isActive, onActivatePane }: EditorTabProps) {
  const workspaceRevision = useGitStore((state) => state.revisions[workspaceId] ?? 0);
  const isTabDirty = useWorkspaceStore(
    (state) => state.dirtyTabs[workspaceId]?.[tabId] === true,
  );
  const [loadState, setLoadState] = useState<EditorLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });
  const requestIdRef = useRef(0);
  const revisionRef = useRef(workspaceRevision);
  const revisionLoadInFlightRef = useRef(false);
  const revisionLoadPendingRef = useRef(false);
  const revisionLoadTargetRef = useRef({ workspaceId, tabId });
  const isTabDirtyRef = useRef(isTabDirty);

  revisionLoadTargetRef.current = { workspaceId, tabId };
  isTabDirtyRef.current = isTabDirty;

  const loadDocument = async (
    targetWorkspaceId: WorkspaceId,
    targetTabId: TabId,
    showLoading: boolean,
  ) => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;

    if (showLoading) {
      setLoadState({ status: "loading", workspaceId: targetWorkspaceId, tabId: targetTabId });
    }

    try {
      const params = {
        workspaceId: targetWorkspaceId,
        tabId: targetTabId,
      };
      const gitLineHunksRequest = getEditorGitLineHunks(params).catch(() => ({ hunks: [] }));
      const document = await getEditorDocument(params);

      if (requestIdRef.current === requestId) {
        setLoadState((current) => ({
          status: "loaded",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          document,
          gitLineHunks:
            current.status === "loaded" &&
            current.workspaceId === targetWorkspaceId &&
            current.tabId === targetTabId
              ? current.gitLineHunks
              : [],
        }));
      }

      const gitLineHunks = await gitLineHunksRequest;
      if (requestIdRef.current === requestId) {
        setLoadState((current) =>
          current.status === "loaded" &&
          current.workspaceId === targetWorkspaceId &&
          current.tabId === targetTabId
            ? { ...current, gitLineHunks: gitLineHunks.hunks }
            : current,
        );
      }
    } catch (caughtError: unknown) {
      if (requestIdRef.current === requestId) {
        setLoadState((current) => {
          if (
            !showLoading &&
            isTabDirtyRef.current &&
            current.status === "loaded" &&
            current.workspaceId === targetWorkspaceId &&
            current.tabId === targetTabId
          ) {
            return current;
          }

          return {
            status: "error",
            workspaceId: targetWorkspaceId,
            tabId: targetTabId,
            message: errorMessage(caughtError),
          };
        });
      }
    }
  };

  const loadDocumentRevision = async () => {
    if (revisionLoadInFlightRef.current) {
      revisionLoadPendingRef.current = true;
      return;
    }

    revisionLoadInFlightRef.current = true;
    try {
      do {
        revisionLoadPendingRef.current = false;
        const target = revisionLoadTargetRef.current;
        await loadDocument(target.workspaceId, target.tabId, false);
      } while (revisionLoadPendingRef.current);
    } finally {
      revisionLoadInFlightRef.current = false;
    }
  };

  useEffect(() => {
    revisionRef.current = workspaceRevision;
    void loadDocument(workspaceId, tabId, true);
  }, [workspaceId, tabId]);

  useEffect(() => {
    if (workspaceRevision === revisionRef.current) {
      return;
    }

    revisionRef.current = workspaceRevision;
    void loadDocumentRevision();
  }, [workspaceRevision, workspaceId, tabId]);

  const currentLoadState: EditorLoadState =
    loadState.workspaceId === workspaceId && loadState.tabId === tabId
      ? loadState
      : { status: "loading", workspaceId, tabId };

  return (
    <div
      className="relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background"
      onPointerDown={onActivatePane}
    >
      {currentLoadState.status === "loading" ? <EditorMessage message="Loading file..." /> : null}
      {currentLoadState.status === "error" ? (
        <EditorMessage message={currentLoadState.message} />
      ) : null}
      {currentLoadState.status === "loaded" ? (
        <LoadedEditor
          workspaceId={workspaceId}
          tabId={tabId}
          document={currentLoadState.document}
          gitLineHunks={currentLoadState.gitLineHunks}
          isActive={isActive}
        />
      ) : null}
    </div>
  );
}

function LoadedEditor({
  workspaceId,
  tabId,
  document,
  gitLineHunks,
  isActive,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  document: EditorDocument;
  gitLineHunks: EditorGitLineHunk[];
  isActive: boolean;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const decorationsRef = useRef<monaco.editor.IEditorDecorationsCollection | null>(null);
  const bufferRef = useRef<EditorBuffer | null>(null);
  const saveCoordinatorRef = useRef(createDocumentSaveCoordinator());
  const formatErrorDocumentRef = useRef(`${workspaceId}:${tabId}:${document.path}`);
  const [saveState, setSaveState] = useState<SaveState>({ status: "clean" });
  const [formatError, setFormatError] = useState<string | null>(null);
  const pendingSelection = useWorkspaceStore((state) => state.pendingEditorSelection);
  const consumePendingEditorSelection = useWorkspaceStore(
    (state) => state.consumePendingEditorSelection,
  );
  const setTabDirty = useWorkspaceStore((state) => state.setTabDirty);
  const bumpGitRevision = useGitStore((state) => state.bumpGitRevision);
  const settings = useSettingsStore((state) => editorSettings(state.snapshot));
  const minimap = settings?.minimap;
  const softWrap = settings?.softWrap;
  const formatOnSave = settings?.formatOnSave;

  useEffect(() => {
    const documentKey = `${workspaceId}:${tabId}:${document.path}`;
    const documentChanged = formatErrorDocumentRef.current !== documentKey;
    formatErrorDocumentRef.current = documentKey;
    setFormatError((current) =>
      formattingErrorAfterContextChange(current, {
        formattingEnabled: formatOnSave === true,
        documentChanged,
      }),
    );
  }, [document.path, formatOnSave, tabId, workspaceId]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !settings) {
      return undefined;
    }

    applyMonacoTheme();
    const uri = monaco.Uri.from({
      scheme: "kosmos",
      authority: `workspace-${workspaceId}`,
      path: `/${document.path}`,
    });
    const buffer = getOrCreateEditorBuffer(
      workspaceId,
      tabId,
      document.path,
      document.content,
      () => monaco.editor.createModel(document.content, undefined, uri),
    );
    bufferRef.current = buffer;
    const { model } = buffer;
    const editor = monaco.editor.create(container, {
      model,
      automaticLayout: true,
      bracketPairColorization: { enabled: true },
      fontSize: 13,
      minimap: { enabled: settings.minimap },
      padding: { top: 8 },
      scrollBeyondLastLine: false,
      smoothScrolling: true,
      theme: "kosmos",
      wordWrap: settings.softWrap ? "on" : "off",
      readOnly: isEditorBufferLocked(buffer),
    });
    editorRef.current = editor;
    decorationsRef.current = editor.createDecorationsCollection();
    const updateDirtyState = () => {
      const isDirty = buffer.model.getValue() !== buffer.savedContent;

      setTabDirty(workspaceId, tabId, isDirty);
      setSaveState(isDirty ? { status: "dirty" } : { status: "clean" });
    };
    const operationCancellations = new Set<monaco.CancellationTokenSource>();
    const beginOperation = () => {
      const cancellation = new monaco.CancellationTokenSource();
      operationCancellations.add(cancellation);
      return cancellation;
    };
    const finishOperation = (cancellation: monaco.CancellationTokenSource) => {
      operationCancellations.delete(cancellation);
      cancellation.dispose();
    };
    const cancelOperations = () => {
      for (const cancellation of operationCancellations) cancellation.cancel();
    };
    let contentSubscription: monaco.IDisposable | null = null;
    const bindModel = (nextModel: monaco.editor.ITextModel) => {
      const viewState = editor.getModel() ? editor.saveViewState() : null;
      if (editor.getModel() !== nextModel) {
        editor.setModel(nextModel);
        if (viewState) {
          editor.restoreViewState(viewState);
        }
      }
      contentSubscription?.dispose();
      contentSubscription = nextModel.onDidChangeContent(() => {
        queueEditorBufferSynchronization(buffer);
        updateDirtyState();
      });
      decorationsRef.current?.set(
        editorGitDecorations(gitLineHunks, nextModel.getLineCount()),
      );
      updateDirtyState();
    };
    bindModel(model);
    void openEditorBufferSession(buffer, document)
      .then(updateDirtyState)
      .catch((caughtError: unknown) => {
        setSaveState({ status: "error", message: errorMessage(caughtError) });
      });
    const unsubscribeModel = subscribeEditorBufferModel(buffer, bindModel);
    const unsubscribeLock = subscribeEditorBufferLock(buffer, (locked) => {
      if (locked) {
        saveCoordinatorRef.current.invalidate();
        cancelOperations();
      }
      editor.updateOptions({ readOnly: locked });
    });
    const save = async () => {
      let model: monaco.editor.ITextModel;
      try {
        assertEditorBufferEditable(buffer);
        model = buffer.model;
      } catch (caughtError: unknown) {
        setSaveState({ status: "error", message: errorMessage(caughtError) });
        return;
      }
      setSaveState({ status: "saving" });
      const cancellation = beginOperation();
      const coordinatedSave = saveCoordinatorRef.current.begin(async () => {
        await flushEditorBuffer(buffer);
        const saved = await saveEditorDocument(
          { workspaceId, tabId, revision: buffer.session.revision },
          cancellation.token,
        );
        buffer.savedContent = saved.savedContent;
      });
      try {
        await coordinatedSave.run(async (isCurrent) => {
          assertEditorBufferEditable(buffer);
          let content = model.getValue();
          let formatted = false;
          if (editorSettings(useSettingsStore.getState().snapshot)?.formatOnSave) {
            try {
              formatted = await formatLanguageDocument(editor, cancellation.token);
              setFormatError(null);
            } catch (caughtError: unknown) {
              const message = errorMessage(caughtError);
              setFormatError((current) => (current === message ? current : message));
            }
          }
          if (!isCurrent()) {
            return;
          }
          if (formatted) {
            const formattedModel = buffer.model;
            const formattedContent = formattedModel.getValue();
            if (formattedContent !== content) {
              await flushEditorBuffer(buffer);
              const saved = await saveEditorDocument(
                { workspaceId, tabId, revision: buffer.session.revision },
                cancellation.token,
              );
              buffer.savedContent = saved.savedContent;
            }
            content = formattedContent;
          }
          const savedModel = buffer.model;
          void notifyLanguageDocumentSaved(savedModel, content).catch(() => {});
          if (!isCurrent()) {
            return;
          }
          updateDirtyState();
          bumpGitRevision(workspaceId);
        });
      } catch (caughtError: unknown) {
        if (coordinatedSave.isCurrent()) {
          setTabDirty(workspaceId, tabId, true);
          setSaveState({ status: "error", message: errorMessage(caughtError) });
        }
      } finally {
        finishOperation(cancellation);
      }
    };
    const saveAction = editor.addAction({
      id: "kosmos.save",
      label: "Save",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
      run: () => {
        void save();
      },
    });
    const formatAction = editor.addAction({
      id: "kosmos.formatDocument",
      label: "Format Document",
      keybindings: [monaco.KeyMod.Shift | monaco.KeyMod.Alt | monaco.KeyCode.KeyF],
      run: async () => {
        const cancellation = beginOperation();
        try {
          assertEditorBufferEditable(buffer);
          await formatLanguageDocument(editor, cancellation.token);
          setFormatError(null);
        } catch (caughtError: unknown) {
          const message = errorMessage(caughtError);
          setFormatError((current) => (current === message ? current : message));
        } finally {
          finishOperation(cancellation);
        }
      },
    });

    return () => {
      saveCoordinatorRef.current.invalidate();
      cancelOperations();
      unsubscribeModel();
      unsubscribeLock();
      contentSubscription?.dispose();
      saveAction.dispose();
      formatAction.dispose();
      decorationsRef.current?.clear();
      decorationsRef.current = null;
      editor.dispose();
      editorRef.current = null;
      if (bufferRef.current === buffer) {
        bufferRef.current = null;
      }
    };
  }, [workspaceId, tabId, document.path, bumpGitRevision, setTabDirty, settings]);

  useEffect(() => {
    const editor = editorRef.current;
    const model = editor?.getModel();
    if (
      !editor ||
      !model ||
      !isActive ||
      !pendingSelection ||
      pendingSelection.tabId !== tabId ||
      pendingSelection.workspaceId !== workspaceId ||
      pendingSelection.path !== document.path ||
      !consumePendingEditorSelection(pendingSelection.generation)
    ) {
      return;
    }
    const start = model.validatePosition({
      lineNumber: pendingSelection.lineNumber,
      column: pendingSelection.column,
    });
    const end = model.validatePosition({
      lineNumber: pendingSelection.endLineNumber,
      column: pendingSelection.endColumn,
    });
    const selection = new monaco.Selection(
      start.lineNumber,
      start.column,
      end.lineNumber,
      end.column,
    );
    editor.setSelection(selection);
    editor.revealRangeInCenter(selection);
    editor.focus();
  }, [
    consumePendingEditorSelection,
    document.path,
    isActive,
    pendingSelection,
    workspaceId,
  ]);

  useEffect(() => {
    const buffer = bufferRef.current;
    if (!buffer || buffer.path !== document.path) {
      return;
    }

    const isDirty = reconcileEditorBuffer(buffer, document.content);
    setTabDirty(workspaceId, tabId, isDirty);
    setSaveState((current) => {
      if (current.status === "saving") {
        return current;
      }
      return isDirty ? { status: "dirty" } : { status: "clean" };
    });
  }, [workspaceId, tabId, document.path, document.content, setTabDirty]);

  useEffect(() => {
    const buffer = bufferRef.current;
    if (!buffer) {
      return;
    }

    decorationsRef.current?.set(editorGitDecorations(gitLineHunks, buffer.model.getLineCount()));
  }, [document.content, gitLineHunks]);

  useEffect(() => {
    if (minimap === undefined || softWrap === undefined) {
      return;
    }
    editorRef.current?.updateOptions({
      minimap: { enabled: minimap },
      wordWrap: softWrap ? "on" : "off",
    });
  }, [minimap, softWrap]);

  useEffect(() => {
    const editor = editorRef.current;
    if (!editor || !isActive) {
      return;
    }

    const frameId = requestAnimationFrame(() => {
      editor.layout();
      editor.focus();
    });

    return () => cancelAnimationFrame(frameId);
  }, [isActive]);

  return (
    <div className="relative h-full min-h-0 min-w-0 overflow-hidden">
      <div ref={containerRef} className="h-full min-h-0 min-w-0" />
      <SaveStatus state={saveState} formatError={formatError} />
    </div>
  );
}

function SaveStatus({ state, formatError }: { state: SaveState; formatError: string | null }) {
  if (state.status === "clean" && formatError === null) {
    return null;
  }

  const saveMessage =
    state.status === "clean"
      ? null
      : state.status === "dirty"
        ? "Unsaved"
        : state.status === "saving"
          ? "Saving..."
          : state.message;

  return (
    <div
      role={state.status === "error" || formatError ? "alert" : "status"}
      className="pointer-events-auto absolute right-3 bottom-3 max-h-40 max-w-[min(32rem,calc(100%-1.5rem))] overflow-auto rounded border border-border/70 bg-popover/95 px-2 py-1 text-xs text-muted-foreground shadow-sm"
    >
      {saveMessage ? <div>{saveMessage}</div> : null}
      {formatError ? (
        <div className="select-text whitespace-pre-wrap break-words" title={formatError}>
          <span className="font-medium text-destructive">Formatting failed: </span>
          {formatError}
        </div>
      ) : null}
    </div>
  );
}

function EditorMessage({ message }: { message: string }) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <p className="text-sm text-muted-foreground">{message}</p>
    </div>
  );
}
