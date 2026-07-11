import { useEffect, useRef, useState } from "react";

import { getEditorDocument, getEditorGitLineHunks, saveEditorDocument } from "@/renderer/ipc";
import {
  getOrCreateEditorBuffer,
  reconcileEditorBuffer,
  type EditorBuffer,
} from "@/renderer/lib/editor-buffers";
import { editorSettings } from "@/renderer/lib/editor-settings";
import { editorGitDecorations } from "@/renderer/lib/editor-git-decorations";
import { errorMessage } from "@/renderer/lib/errors";
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
  const saveRequestIdRef = useRef(0);
  const [saveState, setSaveState] = useState<SaveState>({ status: "clean" });
  const setTabDirty = useWorkspaceStore((state) => state.setTabDirty);
  const bumpGitRevision = useGitStore((state) => state.bumpGitRevision);
  const minimap = useSettingsStore((state) => editorSettings(state.snapshot).minimap);
  const softWrap = useSettingsStore((state) => editorSettings(state.snapshot).softWrap);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
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
    const initialSettings = editorSettings(useSettingsStore.getState().snapshot);
    const editor = monaco.editor.create(container, {
      model,
      automaticLayout: true,
      bracketPairColorization: { enabled: true },
      fontSize: 13,
      minimap: { enabled: initialSettings.minimap },
      padding: { top: 8 },
      scrollBeyondLastLine: false,
      smoothScrolling: true,
      theme: "kosmos",
      wordWrap: initialSettings.softWrap ? "on" : "off",
    });
    editorRef.current = editor;
    decorationsRef.current = editor.createDecorationsCollection(
      editorGitDecorations(gitLineHunks, model.getLineCount()),
    );
    const updateDirtyState = () => {
      const isDirty = model.getValue() !== buffer.savedContent;

      setTabDirty(workspaceId, tabId, isDirty);
      setSaveState(isDirty ? { status: "dirty" } : { status: "clean" });
    };

    updateDirtyState();

    const contentSubscription = model.onDidChangeContent(() => {
      updateDirtyState();
    });
    const saveAction = editor.addAction({
      id: "kosmos.save",
      label: "Save",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
      run: () => {
        const content = model.getValue();
        const requestId = saveRequestIdRef.current + 1;
        saveRequestIdRef.current = requestId;
        setSaveState({ status: "saving" });

        void saveEditorDocument({ workspaceId, tabId, content })
          .then(() => {
            if (saveRequestIdRef.current !== requestId) {
              return;
            }

            buffer.savedContent = content;
            updateDirtyState();
            bumpGitRevision(workspaceId);
          })
          .catch((caughtError: unknown) => {
            if (saveRequestIdRef.current === requestId) {
              setTabDirty(workspaceId, tabId, true);
              setSaveState({ status: "error", message: errorMessage(caughtError) });
            }
          });
      },
    });

    return () => {
      saveRequestIdRef.current += 1;
      contentSubscription.dispose();
      saveAction.dispose();
      decorationsRef.current?.clear();
      decorationsRef.current = null;
      editor.dispose();
      editorRef.current = null;
      if (bufferRef.current === buffer) {
        bufferRef.current = null;
      }
    };
  }, [workspaceId, tabId, document.path, bumpGitRevision, setTabDirty]);

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
      <SaveStatus state={saveState} />
    </div>
  );
}

function SaveStatus({ state }: { state: SaveState }) {
  if (state.status === "clean") {
    return null;
  }

  const message =
    state.status === "dirty"
      ? "Unsaved"
      : state.status === "saving"
        ? "Saving..."
        : state.message;

  return (
    <div
      role={state.status === "error" ? "alert" : "status"}
      className="pointer-events-none absolute right-3 bottom-3 max-w-80 truncate rounded border border-border/70 bg-popover/95 px-2 py-1 text-xs text-muted-foreground shadow-sm"
    >
      {message}
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
