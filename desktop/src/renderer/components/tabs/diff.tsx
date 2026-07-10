import { useEffect, useRef, useState } from "react";

import { Button } from "@/renderer/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/renderer/components/ui/select";
import { getGitDiff, saveGitDiffFile } from "@/renderer/ipc";
import { editorSettings } from "@/renderer/lib/editor-settings";
import { errorMessage } from "@/renderer/lib/errors";
import { applyMonacoTheme, monaco } from "@/renderer/lib/monaco";
import { useGitStore, useSettingsStore, useWorkspaceStore } from "@/renderer/stores";
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

  revisionLoadTargetRef.current = { workspaceId, tabId };

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
        setLoadState({
          status: "error",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          message: errorMessage(caughtError),
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
  const [selectedPath, setSelectedPath] = useState(() => selectedDiffPath(diff));
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const setTabDirty = useWorkspaceStore((state) => state.setTabDirty);
  const updateDirtyState = (dirty: boolean) => {
    setHasUnsavedChanges(dirty);
    setTabDirty(workspaceId, tabId, dirty);
  };

  useEffect(() => {
    if (
      !hasUnsavedChanges &&
      diff.focusedPath &&
      diff.files.some((file) => file.path === diff.focusedPath)
    ) {
      setSelectedPath(diff.focusedPath);
    }
  }, [diff.focusedPath]);

  useEffect(() => {
    setSelectedPath((currentPath) => {
      return diff.files.some((file) => file.path === currentPath)
        ? currentPath
        : (diff.files[0]?.path ?? "");
    });
  }, [diff.files]);

  if (diff.files.length === 0) {
    return <DiffMessage message="No diff" />;
  }

  const file = diff.files.find((candidate) => candidate.path === selectedPath) ?? diff.files[0];
  if (!file) {
    return <DiffMessage message="No diff" />;
  }

  const selectFile = (path: string) => {
    if (hasUnsavedChanges && !window.confirm("Discard unsaved diff edits?")) {
      return;
    }

    updateDirtyState(false);
    setSelectedPath(path);
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-2">
        <Select value={file.path} onValueChange={(path) => path && selectFile(path)}>
          <SelectTrigger size="sm" aria-label="Changed file" className="min-w-0 flex-1 justify-start">
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="start">
            {diff.files.map((candidate) => (
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
  const selectSection = (kind: GitDiffSectionKind) => {
    if (hasUnsavedChanges && !window.confirm("Discard unsaved diff edits?")) {
      return;
    }

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
              onClick={() => selectSection(candidate.kind)}
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
  const modifiedModelRef = useRef<monaco.editor.ITextModel | null>(null);
  const savedContentRef = useRef(section.modifiedContent ?? "");
  const saveRequestIdRef = useRef(0);
  const saveInFlightRef = useRef(false);
  const [saveState, setSaveState] = useState<SaveState>({ status: "clean" });
  const bumpGitRevision = useGitStore((state) => state.bumpGitRevision);
  const softWrap = useSettingsStore((state) => editorSettings(state.snapshot).softWrap);
  const unavailable = section.originalContent == null || section.modifiedContent == null;
  const conflicted = file.staged === "conflicted" || file.unstaged === "conflicted";

  const save = async (stage: boolean) => {
    const model = modifiedModelRef.current;
    if (!model || !section.editable || saveInFlightRef.current) {
      return;
    }

    saveInFlightRef.current = true;
    editorRef.current?.getModifiedEditor().updateOptions({ readOnly: true });
    const content = model.getValue();
    const requestId = saveRequestIdRef.current + 1;
    saveRequestIdRef.current = requestId;
    setSaveState({ status: "saving" });

    try {
      await saveGitDiffFile({ workspaceId, tabId, path: file.path, content, stage });
      if (saveRequestIdRef.current !== requestId) {
        return;
      }

      savedContentRef.current = content;
      const dirty = model.getValue() !== content;
      setSaveState(dirty ? { status: "dirty" } : { status: "clean" });
      onDirtyChange(dirty);
      bumpGitRevision(workspaceId);
    } catch (caughtError: unknown) {
      if (saveRequestIdRef.current === requestId) {
        setSaveState({ status: "error", message: errorMessage(caughtError) });
      }
    } finally {
      if (saveRequestIdRef.current === requestId) {
        saveInFlightRef.current = false;
        editorRef.current?.getModifiedEditor().updateOptions({ readOnly: false });
      }
    }
  };

  useEffect(() => {
    const container = containerRef.current;
    if (!container || unavailable) {
      return undefined;
    }

    applyMonacoTheme();
    const initialSettings = editorSettings(useSettingsStore.getState().snapshot);
    const originalModel = monaco.editor.createModel(
      section.originalContent ?? "",
      undefined,
      diffUri(workspaceId, file.path, section.kind, "original"),
    );
    const modifiedModel = monaco.editor.createModel(
      section.modifiedContent ?? "",
      undefined,
      diffUri(workspaceId, file.path, section.kind, "modified"),
    );
    const editor = monaco.editor.createDiffEditor(container, {
      automaticLayout: true,
      compactMode: true,
      diffAlgorithm: "advanced",
      diffCodeLens: false,
      diffWordWrap: initialSettings.softWrap ? "on" : "off",
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
      readOnly: !section.editable,
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
      wordWrap: initialSettings.softWrap ? "on" : "off",
    });
    editor.setModel({ original: originalModel, modified: modifiedModel });
    editorRef.current = editor;
    originalModelRef.current = originalModel;
    modifiedModelRef.current = modifiedModel;
    savedContentRef.current = section.modifiedContent ?? "";

    const contentSubscription = modifiedModel.onDidChangeContent(() => {
      const dirty = modifiedModel.getValue() !== savedContentRef.current;
      setSaveState(dirty ? { status: "dirty" } : { status: "clean" });
      onDirtyChange(dirty);
    });
    const saveAction = editor.getModifiedEditor().addAction({
      id: "kosmos.save-diff-file",
      label: "Save diff file",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
      run: () => save(false),
    });

    return () => {
      saveRequestIdRef.current += 1;
      contentSubscription.dispose();
      saveAction.dispose();
      editor.dispose();
      originalModel.dispose();
      modifiedModel.dispose();
      editorRef.current = null;
      originalModelRef.current = null;
      modifiedModelRef.current = null;
    };
  }, [workspaceId, tabId, file.path, section.kind, unavailable]);

  useEffect(() => {
    const originalModel = originalModelRef.current;
    const modifiedModel = modifiedModelRef.current;
    if (!originalModel || !modifiedModel || unavailable) {
      return;
    }

    const originalContent = section.originalContent ?? "";
    const modifiedContent = section.modifiedContent ?? "";
    if (originalModel.getValue() !== originalContent) {
      originalModel.setValue(originalContent);
    }
    if (modifiedModel.getValue() === savedContentRef.current) {
      savedContentRef.current = modifiedContent;
      if (modifiedModel.getValue() !== modifiedContent) {
        modifiedModel.setValue(modifiedContent);
      }
      setSaveState({ status: "clean" });
      onDirtyChange(false);
    }
  }, [section.originalContent, section.modifiedContent, unavailable]);

  useEffect(() => {
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
      {section.editable && (saveState.status !== "clean" || conflicted) ? (
        <div className="absolute right-3 bottom-3 flex items-center gap-2 rounded border border-border/70 bg-popover/95 p-1 shadow-sm">
          <SaveStatus state={saveState} />
          {conflicted ? (
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
