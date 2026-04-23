import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { Code, Image as ImageIcon } from "@phosphor-icons/react";
import Editor, { type Monaco } from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import {
  TextDocumentSyncKind,
  type TextDocumentContentChangeEvent,
} from "vscode-languageserver-protocol";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { useLspStore } from "../../store/lsp.store";
import { pathToFileUri } from "../../lib/lsp/uri";
import { useLayoutStore } from "../../store/layout.store";
import { useEditorStore } from "../../store/editor.store";
import { useSettingsStore } from "../../store/settings.store";
import { findLeaf } from "../../lib/paneTree";
import { setupMonacoLanguages, resolveModelLanguage } from "../../lib/lsp/monacoLanguages";
import { useThemeListener } from "../../hooks/useThemeListener";
import { getEditorMeta } from "../../types";
import { StateView } from "../../components/shared/StateView";
import { ContextMenu, type ContextMenuItem } from "../../components/shared/ContextMenu";
import { BASE_EDITOR_OPTIONS } from "../../lib/monacoConfig";
import { initExtMap, languageIdFromExt } from "../../lib/extToLang";
import { normalizePath, getFileExtension, isImagePath } from "../../lib/pathUtils";
import type { TabContentProps } from "../types";
import { defineKosmosTheme } from "./monacoTheme";
import { registerEditorOpener } from "./editorOpener";
import { editorCache } from "./editorCache";
import { parseDiffChanges, buildDiffDecorations } from "./diffDecorations";
import {
  AI_GENERATE_GLYPH_LOADING_CLASS,
  buildAiGutterDecorations,
  buildGenerationPrompt,
  buildWindowedContext,
  extractFunctions,
  stripCodeFences,
  type AiFunctionInfo,
} from "./aiGutter";
import { ImageViewer } from "./ImageViewer";

function languageIdFromPath(filePath: string): string {
  const ext = getFileExtension(filePath);
  return (ext && languageIdFromExt(ext)) ?? "plaintext";
}

export function EditorTab({ tab, paneId }: TabContentProps) {
  const filePath = getEditorMeta(tab)?.filePath;
  const isImage = filePath ? isImagePath(filePath) : false;
  const isSvg = filePath ? getFileExtension(filePath) === "svg" : false;
  const [showCode, setShowCode] = useState(false);

  if (!isImage) {
    return <EditorTabContent tab={tab} paneId={paneId} filePath={filePath} />;
  }

  const showingCode = isSvg && showCode;

  const handleShowPreview = async () => {
    // Flush any unsaved edits so the preview reflects them.
    if (filePath) await editorCache.get(filePath)?.save?.();
    setShowCode(false);
  };

  return (
    <div className="relative h-full">
      {showingCode ? (
        <EditorTabContent tab={tab} paneId={paneId} filePath={filePath} />
      ) : (
        <ImageViewer filePath={filePath!} />
      )}
      {isSvg && (
        <button
          type="button"
          onClick={showingCode ? handleShowPreview : () => setShowCode(true)}
          className="absolute top-2 right-2 z-10 flex items-center gap-1.5 h-7 px-2.5 text-[11px] font-medium bg-[var(--color-bg-surface)] text-[var(--color-text-secondary)] border border-[var(--color-border-secondary)] hover:border-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors rounded cursor-pointer"
          title={showingCode ? "Show image preview" : "Edit SVG source"}
        >
          {showingCode ? <ImageIcon size={12} /> : <Code size={12} />}
          {showingCode ? "Preview" : "Edit code"}
        </button>
      )}
    </div>
  );
}

function EditorTabContent({
  tab,
  paneId,
  filePath,
}: TabContentProps & { filePath: string | undefined }) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const dirty = useLayoutStore((s) => s.dirtyTabs.has(tab.id));
  const setTabDirty = useLayoutStore((s) => s.setTabDirty);
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const contentRef = useRef<string | null>(null);
  // Baseline alternativeVersionId; comparing against current id lets undo clear dirty automatically.
  const savedVersionIdRef = useRef(0);
  const versionRef = useRef(0);
  const changeDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const middleDragCleanupRef = useRef<(() => void) | null>(null);
  const pendingChangesRef = useRef<TextDocumentContentChangeEvent[]>([]);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [editorReady, setEditorReady] = useState(false);
  const lspOpenedRef = useRef(false);
  const diffDecorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null);
  const aiGutterDecorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null);
  const aiFunctionsRef = useRef<Map<number, AiFunctionInfo>>(new Map());
  // Each in-flight generation owns two sticky decorations: a single-line one on the
  // function's first line for the spinner glyph, and a full-range one used to compute
  // the replacement target. Two decorations because glyphMarginClassName paints on every
  // line the range covers, and the replacement needs the full function range.
  const aiInFlightRef = useRef<
    Map<
      number,
      {
        glyph: editor.IEditorDecorationsCollection;
        range: editor.IEditorDecorationsCollection;
      }
    >
  >(new Map());
  const aiGenerationIdRef = useRef(0);
  const aiRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const blameWidgetRef = useRef<editor.IContentWidget | null>(null);
  const blameTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const blameLineRef = useRef<number>(0);

  const aiCompletionEnabled = useSettingsStore((s) => s.values["ai.enableCompletion"] === true);
  const aiAgent = useSettingsStore((s) => (s.values["ai.agent"] as string) ?? "claude-code");

  const editorFontSize = useEditorStore((s) => s.editorFontSize);
  const zoomEditorIn = useEditorStore((s) => s.zoomEditorIn);
  const zoomEditorOut = useEditorStore((s) => s.zoomEditorOut);
  const resetEditorZoom = useEditorStore((s) => s.resetEditorZoom);
  const setLastClickedEditor = useEditorStore((s) => s.setLastClickedEditor);

  const workspace = useActiveWorkspace();
  const startServer = useLspStore((s) => s.startServer);
  const startCompanions = useLspStore((s) => s.startCompanions);
  const getClient = useLspStore((s) => s.getClient);
  const getCompanionClients = useLspStore((s) => s.getCompanionClients);
  const lspLanguageRef = useRef<string>("plaintext");

  const fileUri = filePath ? pathToFileUri(filePath) : null;

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const isExternalUpdateRef = useRef(false);

  // Mirror reactive values into refs so cleanup/listener closures see latest.
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const fileUriRef = useRef(fileUri);
  fileUriRef.current = fileUri;
  const filePathRef = useRef(filePath);
  filePathRef.current = filePath;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const aiCompletionEnabledRef = useRef(aiCompletionEnabled);
  aiCompletionEnabledRef.current = aiCompletionEnabled;
  const aiAgentRef = useRef(aiAgent);
  aiAgentRef.current = aiAgent;

  const loadFile = useCallback(async () => {
    if (!filePath) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<string>("read_file", { path: filePath });
      setContent(result);
      contentRef.current = result;
      // Rebaseline on the rare re-load after mount; normal path baselines in onMount.
      const model = editorRef.current?.getModel();
      if (model) savedVersionIdRef.current = model.getAlternativeVersionId();
      setTabDirty(tab.id, false);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [filePath]);

  useEffect(() => {
    loadFile();
  }, [loadFile]);

  const saveFile = useCallback(async () => {
    if (!filePath || contentRef.current === null) return;
    // Snapshot version id before the async write so mid-write edits aren't baselined.
    const model = editorRef.current?.getModel();
    const savingVersionId = model?.getAlternativeVersionId() ?? 0;
    const savingContent = contentRef.current;
    try {
      await invoke("write_file", { path: filePath, content: savingContent });
      savedVersionIdRef.current = savingVersionId;
      const currentVid = editorRef.current?.getModel()?.getAlternativeVersionId() ?? 0;
      setTabDirty(tab.id, currentVid !== savingVersionId);

      if (workspace && fileUri) {
        const client = getClient(workspace.path, lspLanguageRef.current);
        client?.didSave(fileUri, savingContent);
        for (const companion of getCompanionClients(workspace.path, lspLanguageRef.current)) {
          companion.didSave(fileUri, savingContent);
        }
      }
    } catch (e) {
      setError(String(e));
    }
  }, [filePath, workspace, fileUri, getClient, getCompanionClients]);

  // onKeyDown registers once; ref lets it invoke the latest saveFile closure.
  const saveFileRef = useRef(saveFile);
  saveFileRef.current = saveFile;

  const refreshDiffDecorations = useCallback(async () => {
    const ed = editorRef.current;
    const ws = workspaceRef.current;
    const fp = filePathRef.current;
    if (!ed || !ws || !fp) return;

    const relative = fp.startsWith(ws.path + "/") ? fp.slice(ws.path.length + 1) : fp;

    try {
      const patch = await invoke<string>("git_diff", {
        path: ws.path,
        file: relative,
        staged: false,
      });
      const changes = parseDiffChanges(patch);
      const decorations = buildDiffDecorations(changes);
      diffDecorationsRef.current?.clear();
      diffDecorationsRef.current = ed.createDecorationsCollection(decorations);
    } catch {
      // Untracked file: drop stale decorations.
      diffDecorationsRef.current?.clear();
    }
  }, []);

  useEffect(() => {
    if (editorReady && workspace) refreshDiffDecorations();
  }, [editorReady, workspace, refreshDiffDecorations]);

  const rebuildAiDecorations = useCallback(() => {
    const ed = editorRef.current;
    if (!ed) return;
    // Skip rendering a normal glyph on any line already covered by a loading glyph.
    const inFlightLines = new Set<number>();
    for (const { glyph } of aiInFlightRef.current.values()) {
      const r = glyph.getRanges()[0];
      if (r) inFlightLines.add(r.startLineNumber);
    }
    const decorations = buildAiGutterDecorations(aiFunctionsRef.current, inFlightLines);
    aiGutterDecorationsRef.current?.clear();
    aiGutterDecorationsRef.current = ed.createDecorationsCollection(decorations);
  }, []);

  const refreshAiGutter = useCallback(async () => {
    const ed = editorRef.current;
    const ws = workspaceRef.current;
    const uri = fileUriRef.current;
    if (!ed) return;

    if (!aiCompletionEnabledRef.current || !ws || !uri) {
      aiGutterDecorationsRef.current?.clear();
      aiGutterDecorationsRef.current = null;
      aiFunctionsRef.current = new Map();
      return;
    }

    const client = useLspStore.getState().getClient(ws.path, lspLanguageRef.current);
    if (!client) return;

    try {
      const symbols = await client.documentSymbol(uri);
      aiFunctionsRef.current = extractFunctions(symbols);
      rebuildAiDecorations();
    } catch {
      aiGutterDecorationsRef.current?.clear();
      aiGutterDecorationsRef.current = null;
      aiFunctionsRef.current = new Map();
    }
  }, [rebuildAiDecorations]);

  const scheduleAiGutterRefresh = useCallback(() => {
    if (aiRefreshTimerRef.current != null) clearTimeout(aiRefreshTimerRef.current);
    aiRefreshTimerRef.current = setTimeout(() => {
      aiRefreshTimerRef.current = null;
      refreshAiGutter();
    }, 400);
  }, [refreshAiGutter]);

  const generateFunctionAtLine = useCallback(
    async (startLine: number) => {
      const ed = editorRef.current;
      const model = ed?.getModel();
      const monaco = monacoRef.current;
      const info = aiFunctionsRef.current.get(startLine);
      if (!ed || !model || !monaco || !info) return;
      // One in-flight generation per starting line — re-clicks are no-ops.
      for (const { glyph } of aiInFlightRef.current.values()) {
        const r = glyph.getRanges()[0];
        if (r && r.startLineNumber === startLine) return;
      }

      // Single-line glyph decoration: spinner sits on the function's first line only.
      const glyphCollection = ed.createDecorationsCollection([
        {
          range: {
            startLineNumber: info.range.startLineNumber,
            startColumn: 1,
            endLineNumber: info.range.startLineNumber,
            endColumn: 1,
          },
          options: {
            stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
            glyphMarginClassName: AI_GENERATE_GLYPH_LOADING_CLASS,
            glyphMarginHoverMessage: { value: "Generating…" },
          },
        },
      ]);
      // Full-range decoration: tracks the function body for the replacement edit.
      const rangeCollection = ed.createDecorationsCollection([
        {
          range: info.range,
          options: {
            stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
          },
        },
      ]);

      const genId = ++aiGenerationIdRef.current;
      aiInFlightRef.current.set(genId, { glyph: glyphCollection, range: rangeCollection });
      rebuildAiDecorations();

      try {
        const functionText = model.getValueInRange(info.range);
        const language = model.getLanguageId();
        const fp = filePathRef.current ?? "";
        const context = buildWindowedContext(model, info.range);

        const prompt = buildGenerationPrompt({
          filePath: fp,
          language,
          context,
          functionText,
          functionStartLine: info.range.startLineNumber,
          functionEndLine: info.range.endLineNumber,
        });

        const result = await invoke<{
          text: string;
          stderr: string;
          raw: string;
          tempPath: string;
        }>("ai_generate", {
          prompt,
          agent: aiAgentRef.current,
          cwd: workspaceRef.current?.path ?? null,
        });

        console.groupCollapsed(`[ai_generate] line ${startLine}`);
        console.log("temp file:", result.tempPath);
        console.log("text (from temp file):", result.text);
        if (result.stderr.trim()) console.log("stderr:", result.stderr);
        if (result.raw.trim()) console.log("raw stdout (ignored):", result.raw);
        console.groupEnd();

        // Keep a defensive strip in case the model writes fences despite the protocol.
        const cleaned = stripCodeFences(result.text).trimEnd();
        if (!cleaned) {
          console.warn(
            `[ai_generate] line ${startLine}: agent didn't write anything to the temp file, skipping edit`,
          );
          return;
        }
        if (cleaned === functionText.trimEnd()) {
          console.warn(
            `[ai_generate] line ${startLine}: response identical to source, skipping edit`,
          );
          return;
        }

        const currentRanges = rangeCollection.getRanges();
        const target = currentRanges[0] ?? info.range;
        ed.executeEdits("ai-generate", [{ range: target, text: cleaned }]);
      } catch (err) {
        console.error("AI generation failed:", err);
      } finally {
        glyphCollection.clear();
        rangeCollection.clear();
        aiInFlightRef.current.delete(genId);
        rebuildAiDecorations();
        // Function boundaries likely shifted; refetch symbols.
        scheduleAiGutterRefresh();
      }
    },
    [rebuildAiDecorations, scheduleAiGutterRefresh],
  );

  const generateFunctionAtLineRef = useRef(generateFunctionAtLine);
  generateFunctionAtLineRef.current = generateFunctionAtLine;

  useEffect(() => {
    if (!editorReady) return;
    if (aiCompletionEnabled) {
      refreshAiGutter();
    } else {
      aiGutterDecorationsRef.current?.clear();
      aiGutterDecorationsRef.current = null;
      aiFunctionsRef.current = new Map();
    }
  }, [editorReady, aiCompletionEnabled, workspace, refreshAiGutter]);

  useEffect(() => {
    const unlisten = listen("git-changed", () => {
      refreshDiffDecorations();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refreshDiffDecorations]);

  const clearBlameWidget = useCallback(() => {
    const ed = editorRef.current;
    if (ed && blameWidgetRef.current) {
      ed.removeContentWidget(blameWidgetRef.current);
      blameWidgetRef.current = null;
    }
  }, []);

  const updateBlame = useCallback(
    async (lineNumber: number) => {
      const ed = editorRef.current;
      const ws = workspaceRef.current;
      const fp = filePathRef.current;
      if (!ed || !ws || !fp) return;

      if (lineNumber === blameLineRef.current) return;
      blameLineRef.current = lineNumber;

      const relative = fp.startsWith(ws.path + "/") ? fp.slice(ws.path.length + 1) : fp;

      try {
        const blame = await invoke<string | null>("git_blame_line", {
          path: ws.path,
          file: relative,
          line: lineNumber,
        });

        // Cursor may have moved during the await; bail if so.
        if (blameLineRef.current !== lineNumber) return;

        clearBlameWidget();
        if (blame) {
          const model = ed.getModel();
          const endCol = model ? model.getLineMaxColumn(lineNumber) : 1;
          const domNode = document.createElement("div");
          domNode.className = "git-blame-inline";
          // Numeric ids are Monaco EditorOption (fontSize=61, lineHeight=75).
          domNode.style.fontSize = `${ed.getOption(61) * 0.85}px`;
          domNode.style.lineHeight = `${ed.getOption(75)}px`;
          domNode.textContent = blame;
          const widget: editor.IContentWidget = {
            getId: () => "git-blame-widget",
            getDomNode: () => domNode,
            getPosition: () => ({
              position: { lineNumber, column: endCol },
              preference: [0],
            }),
          };
          blameWidgetRef.current = widget;
          ed.addContentWidget(widget);
        }
      } catch {
        clearBlameWidget();
      }
    },
    [clearBlameWidget],
  );

  useEffect(() => {
    if (!editorReady || !workspace) return;
    const pos = editorRef.current?.getPosition();
    if (pos) updateBlame(pos.lineNumber);
  }, [editorReady, workspace, updateBlame]);

  useEffect(() => {
    editorRef.current?.updateOptions({ fontSize: editorFontSize });
    const widget = blameWidgetRef.current;
    if (widget) {
      const ed = editorRef.current;
      const fs = ed?.getOption(61);
      const lh = ed?.getOption(75);
      if (fs) widget.getDomNode().style.fontSize = `${fs * 0.85}px`;
      if (lh) widget.getDomNode().style.lineHeight = `${lh}px`;
    }
  }, [editorFontSize]);

  const handleThemeChanged = useCallback(() => {
    const monaco = monacoRef.current;
    if (!monaco) return;
    defineKosmosTheme(monaco);
    monaco.editor.setTheme("kosmos");
  }, []);
  useThemeListener(handleThemeChanged);

  // Start LSP once both editor and workspace exist, regardless of mount order.
  useEffect(() => {
    if (!editorReady || !workspace || !fileUri || !monacoRef.current || !editorRef.current) return;
    if (lspOpenedRef.current) return;

    const lspLang = lspLanguageRef.current;
    let cancelled = false;

    startServer(workspace.path, lspLang, filePath ?? null, monacoRef.current).then((client) => {
      if (cancelled || !client || !editorRef.current) return;
      lspOpenedRef.current = true;
      versionRef.current = 1;
      const text = editorRef.current.getValue();
      client.didOpen(fileUri, lspLang, versionRef.current, text);

      if (aiCompletionEnabledRef.current) scheduleAiGutterRefresh();

      // Companions (e.g. tailwindcss) open the same doc alongside the main server.
      startCompanions(workspace.path, lspLang, filePath ?? null, monacoRef.current!).then(() => {
        if (cancelled) return;
        const companions = getCompanionClients(workspace.path, lspLang);
        for (const companion of companions) {
          companion.didOpen(fileUri, lspLang, versionRef.current, text);
        }
      });
    });

    return () => {
      cancelled = true;
    };
  }, [editorReady, workspace, fileUri, startServer, startCompanions, getCompanionClients]);

  // Unmount: flush pending didChange, send didClose, dispose editor resources.
  useEffect(() => {
    return () => {
      const ws = workspaceRef.current;
      const uri = fileUriRef.current;
      const fp = filePathRef.current;

      if (debounceTimerRef.current != null) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
      if (aiRefreshTimerRef.current != null) {
        clearTimeout(aiRefreshTimerRef.current);
        aiRefreshTimerRef.current = null;
      }
      if (pendingChangesRef.current.length > 0 && ws && uri) {
        const state = useLspStore.getState();
        const client = state.getClient(ws.path, lspLanguageRef.current);
        if (client) {
          client.didChange(uri, versionRef.current, pendingChangesRef.current);
        }
        for (const companion of state.getCompanionClients(ws.path, lspLanguageRef.current)) {
          companion.didChange(uri, versionRef.current, pendingChangesRef.current);
        }
        pendingChangesRef.current = [];
      }

      if (blameTimerRef.current != null) {
        clearTimeout(blameTimerRef.current);
        blameTimerRef.current = null;
      }
      clearBlameWidget();

      lspOpenedRef.current = false;
      changeDisposableRef.current?.dispose();
      middleDragCleanupRef.current?.();
      middleDragCleanupRef.current = null;
      useLayoutStore.getState().setTabDirty(tab.id, false);
      if (fp) {
        editorCache.delete(fp);
        const es = useEditorStore.getState();
        if (es.lastClickedEditorFilePath === fp) es.setLastClickedEditor(null);
      }
      if (ws && uri) {
        const state = useLspStore.getState();
        const client = state.getClient(ws.path, lspLanguageRef.current);
        client?.didClose(uri);
        for (const companion of state.getCompanionClients(ws.path, lspLanguageRef.current)) {
          companion.didClose(uri);
        }
      }
    };
  }, []);

  const isActiveTab = useLayoutStore((s) => {
    const leaf = findLeaf(s.layout, paneId);
    return leaf?.activeTabId === tab.id;
  });

  // Tabs stay mounted inside inert panes; refocus after switching so per-editor
  // key bindings fire. rAF waits for inert to be removed (focus() no-ops on inert).
  // Skip if focus is already inside to avoid stealing from widgets.
  useEffect(() => {
    if (!isActiveTab) return;
    const raf = requestAnimationFrame(() => {
      const ed = editorRef.current;
      if (!ed) return;
      const active = document.activeElement;
      const editorDom = ed.getDomNode();
      const focusAlreadyInside = editorDom && active && editorDom.contains(active);
      if (!focusAlreadyInside) ed.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [isActiveTab]);

  // Reload from disk on external change, but only while the editor is clean.
  useEffect(() => {
    if (!filePath) return;

    const unlisten = listen<string[]>("file-content-changed", async (event) => {
      const changedFiles = event.payload;
      const normFilePath = normalizePath(filePath);
      if (!changedFiles.some((f) => normalizePath(f) === normFilePath)) return;

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
          setTabDirty(tab.id, false);
        } else {
          setContent(newContent);
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
  }, [filePath]);

  function handleEditorDidMount(instance: editor.IStandaloneCodeEditor, monaco: Monaco) {
    editorRef.current = instance;
    monacoRef.current = monaco;

    // Remeasure once webfonts swap in to avoid cursor-glyph offset.
    document.fonts.ready.then(() => {
      if (editorRef.current) monaco.editor.remeasureFonts();
    });

    // Pick the narrowest registered language (e.g. typescriptreact over typescript).
    const model = instance.getModel();
    if (model) resolveModelLanguage(monaco, model);
    lspLanguageRef.current = model?.getLanguageId() ?? "plaintext";

    savedVersionIdRef.current = model?.getAlternativeVersionId() ?? 0;
    useLayoutStore.getState().setTabDirty(tab.id, false);

    setEditorReady(true);

    instance.focus();

    if (filePath) {
      const cached = editorCache.get(filePath);
      const pendingReveal = cached?.pendingReveal;
      editorCache.set(filePath, {
        instance,
        pendingReveal: undefined,
        save: () => saveFileRef.current(),
      });
      setLastClickedEditor(filePath);
      if (pendingReveal) {
        // @monaco-editor/react flips display:none→block after onMount, triggering a
        // ResizeObserver layout that resets scroll; defer reveal past it.
        setTimeout(() => {
          instance.setPosition(pendingReveal);
          instance.revealPositionInCenter(pendingReveal);
        }, 50);
      }
    }

    instance.onDidFocusEditorWidget(() => {
      if (filePath) setLastClickedEditor(filePath);
    });

    // Use per-instance onKeyDown (not addCommand) so multiple editors don't
    // overwrite each other in Monaco's global keybinding registry.
    instance.onKeyDown((e) => {
      const ctrl = e.ctrlKey || e.metaKey;
      if (!ctrl || e.shiftKey || e.altKey) return;
      if (e.keyCode === monaco.KeyCode.KeyS) {
        e.preventDefault();
        e.stopPropagation();
        saveFileRef.current();
        return;
      }
      if (e.keyCode === monaco.KeyCode.Equal) {
        e.preventDefault();
        e.stopPropagation();
        zoomEditorIn();
        return;
      }
      if (e.keyCode === monaco.KeyCode.Minus) {
        e.preventDefault();
        e.stopPropagation();
        zoomEditorOut();
        return;
      }
      if (e.keyCode === monaco.KeyCode.Digit0) {
        e.preventDefault();
        e.stopPropagation();
        resetEditorZoom();
        return;
      }
    });

    // On Linux middle-click pastes PRIMARY; suppress paste when a middle-drag selects
    // text so the selection isn't overwritten on mouseup. Plain click still pastes.
    const editorDom = instance.getDomNode();
    if (editorDom) {
      let middleDragged = false;
      let middleDownX = 0;
      let middleDownY = 0;
      const DRAG_THRESHOLD = 3;

      const onMouseDown = (e: MouseEvent) => {
        if (e.button !== 1) return;
        middleDragged = false;
        middleDownX = e.clientX;
        middleDownY = e.clientY;
      };
      const onMouseMove = (e: MouseEvent) => {
        if (!(e.buttons & 4)) return;
        if (
          Math.abs(e.clientX - middleDownX) > DRAG_THRESHOLD ||
          Math.abs(e.clientY - middleDownY) > DRAG_THRESHOLD
        ) {
          middleDragged = true;
        }
      };
      const onMouseUp = (e: MouseEvent) => {
        if (e.button !== 1 || !middleDragged) return;
        e.preventDefault();
        e.stopPropagation();
      };

      editorDom.addEventListener("mousedown", onMouseDown, true);
      editorDom.addEventListener("mousemove", onMouseMove, true);
      editorDom.addEventListener("mouseup", onMouseUp, true);
      editorDom.addEventListener("auxclick", onMouseUp, true);

      middleDragCleanupRef.current = () => {
        editorDom.removeEventListener("mousedown", onMouseDown, true);
        editorDom.removeEventListener("mousemove", onMouseMove, true);
        editorDom.removeEventListener("mouseup", onMouseUp, true);
        editorDom.removeEventListener("auxclick", onMouseUp, true);
      };
    }

    instance.onDidChangeCursorPosition((e) => {
      if (blameTimerRef.current != null) clearTimeout(blameTimerRef.current);
      clearBlameWidget();
      blameLineRef.current = 0;
      blameTimerRef.current = setTimeout(() => {
        blameTimerRef.current = null;
        updateBlame(e.position.lineNumber);
      }, 500);
    });

    // Debounce LSP didChange so keystrokes don't flood the server.
    const DIDCHANGE_DEBOUNCE_MS = 200;

    changeDisposableRef.current = instance.onDidChangeModelContent((e) => {
      contentRef.current = instance.getValue();
      // External reload: don't flip dirty.
      if (isExternalUpdateRef.current) return;

      const m = instance.getModel();
      const vid = m?.getAlternativeVersionId() ?? 0;
      const shouldBeDirty = vid !== savedVersionIdRef.current;
      const store = useLayoutStore.getState();
      if (shouldBeDirty !== store.dirtyTabs.has(tab.id)) {
        store.setTabDirty(tab.id, shouldBeDirty);
      }

      if (!workspace || !fileUri) return;
      const client = getClient(workspace.path, lspLanguageRef.current);
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
        const currentClient = getClient(workspace.path, lspLanguageRef.current);
        currentClient?.didChange(fileUri, versionRef.current, pendingChangesRef.current);
        for (const companion of getCompanionClients(workspace.path, lspLanguageRef.current)) {
          companion.didChange(fileUri, versionRef.current, pendingChangesRef.current);
        }
        pendingChangesRef.current = [];
      }, DIDCHANGE_DEBOUNCE_MS);

      if (aiCompletionEnabledRef.current) scheduleAiGutterRefresh();
    });

    instance.onMouseDown((e) => {
      if (!aiCompletionEnabledRef.current) return;
      if (e.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return;
      const line = e.target.position?.lineNumber;
      if (!line) return;
      if (aiFunctionsRef.current.has(line)) {
        e.event.preventDefault();
        e.event.stopPropagation();
        generateFunctionAtLineRef.current(line);
      }
    });
  }

  function handleBeforeMount(monaco: Monaco) {
    monacoRef.current = monaco;
    defineKosmosTheme(monaco);
    setupMonacoLanguages(monaco);
    initExtMap(monaco);
    registerEditorOpener(monaco);

    // Eagerly spawn LSP during editor DOM setup so providers are ready sooner.
    if (workspace && filePath) {
      const lang = languageIdFromPath(filePath);
      lspLanguageRef.current = lang;
      startServer(workspace.path, lang, filePath, monaco);
    }

    // Monaco's built-in TS/JS checks run without tsconfig/node_modules — always false
    // positives. Real diagnostics come from the LSP server.
    monaco.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
      noSemanticValidation: true,
      noSyntaxValidation: true,
    });
    monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
      noSemanticValidation: true,
      noSyntaxValidation: true,
    });
  }

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  const contextMenuItems: ContextMenuItem[] = (() => {
    const ed = editorRef.current;
    const hasSelection = ed ? !ed.getSelection()?.isEmpty() : false;
    return [
      {
        label: "Cut",
        disabled: !hasSelection,
        onClick: () => {
          if (!ed) return;
          const sel = ed.getSelection();
          if (!sel || sel.isEmpty()) return;
          const text = ed.getModel()!.getValueInRange(sel);
          navigator.clipboard.writeText(text);
          ed.executeEdits("context-menu", [{ range: sel, text: "" }]);
          ed.focus();
        },
      },
      {
        label: "Copy",
        disabled: !hasSelection,
        onClick: () => {
          if (!ed) return;
          const sel = ed.getSelection();
          if (!sel || sel.isEmpty()) return;
          navigator.clipboard.writeText(ed.getModel()!.getValueInRange(sel));
          ed.focus();
        },
      },
      {
        label: "Paste",
        onClick: async () => {
          if (!ed) return;
          try {
            const text = await readText();
            if (text) {
              ed.trigger("context-menu", "type", { text });
            }
          } catch {
            /* clipboard empty or inaccessible */
          }
          ed.focus();
        },
      },
      { separator: true as const },
      {
        label: "Select All",
        onClick: () => {
          if (!ed) return;
          const model = ed.getModel();
          if (!model) return;
          const lastLine = model.getLineCount();
          const lastCol = model.getLineMaxColumn(lastLine);
          ed.setSelection({
            startLineNumber: 1,
            startColumn: 1,
            endLineNumber: lastLine,
            endColumn: lastCol,
          });
          ed.focus();
        },
      },
    ];
  })();

  if (!filePath) {
    return <StateView message="No file path" />;
  }

  if (loading) {
    return <StateView message="Loading..." variant="secondary" />;
  }

  if (error) {
    return <StateView message={error} variant="error" />;
  }

  return (
    <div className="flex flex-col h-full" onContextMenu={handleContextMenu}>
      <div className="flex-1 min-h-0">
        <Editor
          path={fileUri ?? undefined}
          defaultValue={content ?? ""}
          theme="kosmos"
          beforeMount={handleBeforeMount}
          onMount={handleEditorDidMount}
          options={{
            ...BASE_EDITOR_OPTIONS,
            fontSize: editorFontSize,
            renderLineHighlight: "line",
            cursorBlinking: "smooth",
            cursorSmoothCaretAnimation: "on",
            bracketPairColorization: { enabled: true },
            guides: {
              indentation: true,
              bracketPairs: false,
            },
            hover: { above: false },
            glyphMargin: aiCompletionEnabled,
          }}
        />
      </div>
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenuItems}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
