import { useEffect, useRef, useState } from "react";

import { Button } from "@/renderer/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/renderer/components/ui/select";
import { getGitDiff, saveEditorDocument, saveGitDiffFile } from "@/renderer/ipc";
import {
  applyEditorSaveProjection,
  closeEditorBufferSession,
  editorBuffer,
  flushEditorBuffer,
  getOrCreateEditorBuffer,
  isEditorBufferDirty,
  queueEditorBufferSynchronization,
  resetEditorBufferSession,
  startEditorBufferSession,
  type EditorBuffer,
} from "@/renderer/lib/editor-buffers";
import { editorSettings } from "@/renderer/lib/editor-settings";
import { errorMessage } from "@/renderer/lib/errors";
import { applyMonacoTheme, monaco } from "@/renderer/lib/monaco";
import { confirmDialog, useGitStore, useSettingsStore, useWorkspaceStore } from "@/renderer/stores";
import type {
  GitChangeKind,
  GitDiff,
  GitDiffFile,
  GitDiffSection,
  GitDiffSectionKind,
  TabId,
  WorkspaceId,
} from "@/shared/ipc";

type DiffTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  isActive: boolean;
  onActivatePane(): void;
};

type DiffLoadState =
  | { status: "loading"; workspaceId: WorkspaceId; tabId: TabId }
  | { status: "loaded"; workspaceId: WorkspaceId; tabId: TabId; diff: GitDiff }
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string };

type SaveState =
  | { status: "clean" }
  | { status: "dirty" }
  | { status: "saving" }
  | { status: "error"; message: string };

export function DiffTab({ workspaceId, tabId, isActive, onActivatePane }: DiffTabProps) {
  const gitRevision = useGitStore((state) => state.revisions[workspaceId] ?? 0);
  const isTabDirty = useWorkspaceStore(
    (state) => state.dirtyTabs[workspaceId]?.[tabId] === true,
  );
  const [loadState, setLoadState] = useState<DiffLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });
  const requestIdRef = useRef(0);
  const revisionRef = useRef(gitRevision);
  const revisionLoadInFlightRef = useRef(false);
  const revisionLoadPendingRef = useRef(false);
  const revisionLoadTargetRef = useRef({ workspaceId, tabId });
  const isTabDirtyRef = useRef(isTabDirty);

  revisionLoadTargetRef.current = { workspaceId, tabId };
  isTabDirtyRef.current = isTabDirty;

  const loadDiff = async (targetWorkspaceId: WorkspaceId, targetTabId: TabId, showLoading: boolean) => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;

    if (showLoading) {
      setLoadState({ status: "loading", workspaceId: targetWorkspaceId, tabId: targetTabId });
    }

    try {
      const diff = await getGitDiff({ workspaceId: targetWorkspaceId, tabId: targetTabId });

      if (requestIdRef.current === requestId) {
        setLoadState({ status: "loaded", workspaceId: targetWorkspaceId, tabId: targetTabId, diff });
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

  const loadDiffRevision = async () => {
    if (revisionLoadInFlightRef.current) {
      revisionLoadPendingRef.current = true;
      return;
    }

    revisionLoadInFlightRef.current = true;
    try {
      do {
        revisionLoadPendingRef.current = false;
        const target = revisionLoadTargetRef.current;
        await loadDiff(target.workspaceId, target.tabId, false);
      } while (revisionLoadPendingRef.current);
    } finally {
      revisionLoadInFlightRef.current = false;
    }
  };

  useEffect(() => {
    revisionRef.current = gitRevision;
    void loadDiff(workspaceId, tabId, true);
  }, [workspaceId, tabId]);

  useEffect(() => {
    if (gitRevision === revisionRef.current) {
      return;
    }

    revisionRef.current = gitRevision;
    void loadDiffRevision();
  }, [gitRevision, workspaceId, tabId]);

  const currentLoadState: DiffLoadState =
    loadState.workspaceId === workspaceId && loadState.tabId === tabId
      ? loadState
      : { status: "loading", workspaceId, tabId };

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background"
      onPointerDown={onActivatePane}
    >
      {currentLoadState.status === "loading" ? <DiffMessage message="Loading diff..." /> : null}
      {currentLoadState.status === "error" ? <DiffMessage message={currentLoadState.message} /> : null}
      {currentLoadState.status === "loaded" ? (
        <LoadedDiff
          workspaceId={workspaceId}
          tabId={tabId}
          diff={currentLoadState.diff}
          isActive={isActive}
        />
      ) : null}
    </div>
  );
}

function LoadedDiff({
  workspaceId,
  tabId,
  diff,
  isActive,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  diff: GitDiff;
  isActive: boolean;
}) {
  const [displayedDiff, setDisplayedDiff] = useState(diff);
  const [selectedPath, setSelectedPath] = useState(() => selectedDiffPath(displayedDiff));
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const setTabDirty = useWorkspaceStore((state) => state.setTabDirty);
  const updateDirtyState = (dirty: boolean) => {
    setHasUnsavedChanges(dirty);
    setTabDirty(workspaceId, tabId, dirty);
  };

  useEffect(() => {
    if (!hasUnsavedChanges) {
      setDisplayedDiff(diff);
    }
  }, [diff, hasUnsavedChanges]);

  useEffect(() => {
    if (
      !hasUnsavedChanges &&
      displayedDiff.focusedPath &&
      displayedDiff.files.some((file) => file.path === displayedDiff.focusedPath)
    ) {
      setSelectedPath(displayedDiff.focusedPath);
    }
  }, [displayedDiff, hasUnsavedChanges]);

  useEffect(() => {
    setSelectedPath((currentPath) => {
      return displayedDiff.files.some((file) => file.path === currentPath)
        ? currentPath
        : (displayedDiff.files[0]?.path ?? "");
    });
  }, [displayedDiff.files]);

  if (displayedDiff.files.length === 0) {
    return <DiffMessage message="No diff" />;
  }

  const file =
    displayedDiff.files.find((candidate) => candidate.path === selectedPath) ??
    displayedDiff.files[0];
  if (!file) {
    return <DiffMessage message="No diff" />;
  }

  const selectFile = async (path: string) => {
    if (hasUnsavedChanges && !(await confirmDiscardDiffEdits())) {
      return;
    }

    await discardDiffDraft(workspaceId, tabId);
    updateDirtyState(false);
    setSelectedPath(path);
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-2">
        <Select value={file.path} onValueChange={(path) => path && void selectFile(path)}>
          <SelectTrigger size="sm" aria-label="Changed file" className="min-w-0 flex-1 justify-start">
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="start">
            {displayedDiff.files.map((candidate) => (
              <SelectItem key={candidate.path} value={candidate.path}>
                <span className="min-w-0 truncate">{candidate.path}</span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <ChangeBadge kind={file.staged} label="staged" />
      </div>
      <DiffFileEditor
        key={file.path}
        workspaceId={workspaceId}
        tabId={tabId}
        file={file}
        isActive={isActive}
        onDirtyChange={updateDirtyState}
      />
    </div>
  );
}

/** Drops the tab's draft, including any unsaved edits the user chose to discard. */
function discardDiffDraft(workspaceId: WorkspaceId, tabId: TabId): Promise<void> {
  return editorBuffer(workspaceId, tabId)
    ? closeEditorBufferSession(workspaceId, tabId)
    : Promise.resolve();
}

function confirmDiscardDiffEdits(): Promise<boolean> {
  return confirmDialog({
    title: "Discard unsaved diff edits?",
    confirmLabel: "Discard",
    destructive: true,
  });
}

function DiffFileEditor({
  workspaceId,
  tabId,
  file,
  isActive,
  onDirtyChange,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  file: GitDiffFile;
  isActive: boolean;
  onDirtyChange(dirty: boolean): void;
}) {
  const [sectionKind, setSectionKind] = useState<GitDiffSectionKind>(() => preferredSection(file).kind);
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const section = file.sections.find((candidate) => candidate.kind === sectionKind) ?? preferredSection(file);
  const selectSection = async (kind: GitDiffSectionKind) => {
    if (hasUnsavedChanges && !(await confirmDiscardDiffEdits())) {
      return;
    }

    await discardDiffDraft(workspaceId, tabId);
    setHasUnsavedChanges(false);
    onDirtyChange(false);
    setSectionKind(kind);
  };
  const updateDirtyState = (dirty: boolean) => {
    setHasUnsavedChanges(dirty);
    onDirtyChange(dirty);
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      {file.sections.length > 1 ? (
        <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border px-2">
          {file.sections.map((candidate) => (
            <Button
              key={candidate.kind}
              type="button"
              size="sm"
              variant={candidate.kind === section.kind ? "secondary" : "ghost"}
              className="h-7 text-xs"
              onClick={() => void selectSection(candidate.kind)}
            >
              {sectionLabel(candidate.kind)}
            </Button>
          ))}
        </div>
      ) : null}
      <MonacoDiffEditor
        key={section.kind}
        workspaceId={workspaceId}
        tabId={tabId}
        file={file}
        section={section}
        isActive={isActive}
        onDirtyChange={updateDirtyState}
      />
    </div>
  );
}

function MonacoDiffEditor({
  workspaceId,
  tabId,
  file,
  section,
  isActive,
  onDirtyChange,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  file: GitDiffFile;
  section: GitDiffSection;
  isActive: boolean;
  onDirtyChange(dirty: boolean): void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);
  const originalModelRef = useRef<monaco.editor.ITextModel | null>(null);
  const draftRef = useRef<EditorBuffer | null>(null);
  const plainModelRef = useRef<monaco.editor.ITextModel | null>(null);
  const saveInFlightRef = useRef(false);
  const [saveState, setSaveState] = useState<SaveState>({ status: "clean" });
  const bumpGitRevision = useGitStore((state) => state.bumpGitRevision);
  const softWrap = useSettingsStore((state) => editorSettings(state.snapshot)?.softWrap);
  const unavailable = section.originalContent == null || section.modifiedContent == null;
  const conflicted = file.staged === "conflicted" || file.unstaged === "conflicted";

  const reportDirtyState = () => {
    const draft = draftRef.current;
    const dirty = draft !== null && isEditorBufferDirty(draft);
    setSaveState((current) =>
      current.status === "saving" ? current : dirty ? { status: "dirty" } : { status: "clean" },
    );
    onDirtyChange(dirty);
  };

  const save = async (stage: boolean) => {
    const draft = draftRef.current;
    if (!draft || saveInFlightRef.current) {
      return;
    }

    saveInFlightRef.current = true;
    setSaveState({ status: "saving" });
    try {
      await flushEditorBuffer(draft);
      const saved = await saveEditorDocument({
        workspaceId,
        tabId,
        revision: draft.session.revision,
      });
      applyEditorSaveProjection(draft, saved);
      if (stage) {
        await saveGitDiffFile({
          workspaceId,
          tabId,
          path: file.path,
          content: draft.model.getValue(),
          stage,
        });
      }
      setSaveState({ status: "clean" });
      reportDirtyState();
      bumpGitRevision(workspaceId);
    } catch (caughtError: unknown) {
      setSaveState({ status: "error", message: errorMessage(caughtError) });
    } finally {
      saveInFlightRef.current = false;
    }
  };

  useEffect(() => {
    const container = containerRef.current;
    if (!container || unavailable || softWrap === undefined) {
      return undefined;
    }

    applyMonacoTheme();
    const originalModel = monaco.editor.createModel(
      section.originalContent ?? "",
      undefined,
      diffUri(workspaceId, file.path, section.kind, "original"),
    );
    const createModifiedModel = () =>
      monaco.editor.createModel(
        section.modifiedContent ?? "",
        undefined,
        diffUri(workspaceId, file.path, section.kind, "modified"),
      );
    // Editable diffs are drafts tracked by a server session, so unsaved edits count
    // in every unsaved-changes check; read-only diffs use a plain model.
    const draft = section.editable
      ? diffDraftBuffer(workspaceId, tabId, file.path, section.modifiedContent ?? "", createModifiedModel)
      : null;
    const modifiedModel = draft?.model ?? createModifiedModel();
    const editor = monaco.editor.createDiffEditor(container, {
      automaticLayout: true,
      compactMode: true,
      diffAlgorithm: "advanced",
      diffCodeLens: false,
      diffWordWrap: softWrap ? "on" : "off",
      enableSplitViewResizing: false,
      experimental: { useTrueInlineView: true },
      folding: false,
      fontSize: 13,
      glyphMargin: false,
      hideUnchangedRegions: {
        contextLineCount: 3,
        enabled: true,
        minimumLineCount: 8,
        revealLineCount: 5,
      },
      minimap: { enabled: false },
      originalEditable: false,
      padding: { top: 8 },
      readOnly: draft === null,
      renderGutterMenu: false,
      renderIndicators: false,
      renderLineHighlight: "none",
      renderMarginRevertIcon: false,
      renderOverviewRuler: false,
      renderSideBySide: false,
      scrollBeyondLastLine: false,
      smoothScrolling: true,
      stickyScroll: { enabled: false },
      theme: "kosmos",
      wordWrap: softWrap ? "on" : "off",
    });
    // Closing the tab disposes the draft before React unmounts this view. The diff
    // widget must let go of its models first, so this listener has to be registered
    // before the widget registers its own.
    const disposalSubscription = modifiedModel.onWillDispose(() => editor.setModel(null));
    editor.setModel({ original: originalModel, modified: modifiedModel });
    editorRef.current = editor;
    originalModelRef.current = originalModel;
    draftRef.current = draft;
    plainModelRef.current = draft ? null : modifiedModel;
    reportDirtyState();

    const contentSubscription = modifiedModel.onDidChangeContent((event) => {
      if (draft) {
        queueEditorBufferSynchronization(draft, event);
      }
      reportDirtyState();
    });
    const saveAction = editor.getModifiedEditor().addAction({
      id: "kosmos.save-diff-file",
      label: "Save diff file",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
      run: () => save(false),
    });

    return () => {
      disposalSubscription.dispose();
      contentSubscription.dispose();
      saveAction.dispose();
      editor.dispose();
      originalModel.dispose();
      // Drafts outlive the view (for example, when the tab moves to another pane).
      if (!draft) {
        modifiedModel.dispose();
      }
      editorRef.current = null;
      originalModelRef.current = null;
      draftRef.current = null;
      plainModelRef.current = null;
    };
  }, [workspaceId, tabId, file.path, section.kind, softWrap, unavailable]);

  useEffect(() => {
    // A file that stops being editable (for example, deleted on disk) must not accept
    // edits that could no longer be saved in place.
    editorRef.current
      ?.getModifiedEditor()
      .updateOptions({ readOnly: !section.editable || draftRef.current === null });
  }, [section.editable]);

  useEffect(() => {
    const originalModel = originalModelRef.current;
    if (!originalModel || unavailable) {
      return;
    }

    const originalContent = section.originalContent ?? "";
    const modifiedContent = section.modifiedContent ?? "";
    if (originalModel.getValue() !== originalContent) {
      originalModel.setValue(originalContent);
    }
    const draft = draftRef.current;
    if (draft) {
      if (!isEditorBufferDirty(draft) && draft.savedContent !== modifiedContent) {
        void resetEditorBufferSession(draft, modifiedContent).then(reportDirtyState);
      }
      return;
    }
    const plainModel = plainModelRef.current;
    if (plainModel && plainModel.getValue() !== modifiedContent) {
      plainModel.setValue(modifiedContent);
    }
  }, [section.originalContent, section.modifiedContent, unavailable]);

  useEffect(() => {
    if (softWrap === undefined) {
      return;
    }
    editorRef.current?.updateOptions({
      diffWordWrap: softWrap ? "on" : "off",
      wordWrap: softWrap ? "on" : "off",
    });
  }, [softWrap]);

  useEffect(() => {
    const editor = editorRef.current;
    if (!editor || !isActive) {
      return;
    }

    const frameId = requestAnimationFrame(() => {
      editor.layout();
      editor.getModifiedEditor().focus();
    });
    return () => cancelAnimationFrame(frameId);
  }, [isActive]);

  if (unavailable) {
    return <DiffMessage message="This binary or oversized file cannot be displayed in Monaco." />;
  }

  return (
    <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden">
      <div ref={containerRef} className="h-full min-h-0 min-w-0" />
      {saveState.status !== "clean" || (section.editable && conflicted) ? (
        <div className="absolute right-3 bottom-3 flex items-center gap-2 rounded border border-border/70 bg-popover/95 p-1 shadow-sm">
          <SaveStatus state={saveState} />
          {section.editable && conflicted ? (
            <Button
              type="button"
              size="sm"
              className="h-7 text-xs"
              disabled={saveState.status === "saving"}
              onClick={() => void save(true)}
            >
              Mark resolved
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/**
 * Returns the diff tab's draft buffer for `path`, restarting the tab's server session
 * when the previous draft was for another file (its edits were already discarded).
 */
function diffDraftBuffer(
  workspaceId: WorkspaceId,
  tabId: TabId,
  path: string,
  content: string,
  createModel: () => monaco.editor.ITextModel,
): EditorBuffer {
  const previous = editorBuffer(workspaceId, tabId);
  const closing =
    previous && previous.path !== path
      ? closeEditorBufferSession(workspaceId, tabId)
      : Promise.resolve();
  const draft = getOrCreateEditorBuffer(workspaceId, tabId, path, content, createModel, {
    languageFeatures: false,
  });
  if (draft !== previous) {
    void closing.then(() => startEditorBufferSession(draft)).catch(() => {});
  }
  return draft;
}

function selectedDiffPath(diff: GitDiff): string {
  if (diff.focusedPath && diff.files.some((file) => file.path === diff.focusedPath)) {
    return diff.focusedPath;
  }
  return diff.files[0]?.path ?? "";
}

function preferredSection(file: GitDiffFile): GitDiffSection {
  const section = file.sections.find((candidate) => candidate.kind === "unstaged") ?? file.sections[0];
  if (!section) {
    throw new Error(`Diff file ${file.path} has no sections.`);
  }
  return section;
}

function diffUri(
  workspaceId: WorkspaceId,
  path: string,
  section: GitDiffSectionKind,
  side: "original" | "modified",
): monaco.Uri {
  return monaco.Uri.from({
    scheme: "kosmos-diff",
    authority: `workspace-${workspaceId}`,
    path: `/${path}`,
    query: `${section}-${side}`,
  });
}

function sectionLabel(kind: GitDiffSectionKind): string {
  return kind === "staged" ? "Staged" : "Working tree";
}

function ChangeBadge({ kind, label }: { kind?: GitChangeKind | null; label: string }) {
  if (!kind) {
    return null;
  }
  return (
    <span className="shrink-0 rounded bg-secondary px-1.5 py-0.5 text-[10px] text-secondary-foreground">
      {label}: {kind}
    </span>
  );
}

function SaveStatus({ state }: { state: SaveState }) {
  if (state.status === "clean") {
    return null;
  }
  const message =
    state.status === "dirty"
      ? "Unsaved (Ctrl+S)"
      : state.status === "saving"
        ? "Saving..."
        : state.message;
  return (
    <span
      role={state.status === "error" ? "alert" : "status"}
      className="max-w-64 truncate px-1 text-xs text-muted-foreground"
    >
      {message}
    </span>
  );
}

function DiffMessage({ message }: { message: string }) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <p className="text-sm text-muted-foreground">{message}</p>
    </div>
  );
}
