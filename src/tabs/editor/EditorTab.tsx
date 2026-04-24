import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Code, Image as ImageIcon } from "@phosphor-icons/react";
import Editor, { type Monaco } from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import type { TextDocumentContentChangeEvent } from "vscode-languageserver-protocol";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { useLspStore } from "../../store/lsp.store";
import { pathToFileUri } from "../../lib/lsp/uri";
import { useLayoutStore } from "../../store/layout.store";
import { useEditorStore } from "../../store/editor.store";
import { findLeaf } from "../../lib/paneTree";
import { setupMonacoLanguages, resolveModelLanguage } from "../../lib/lsp/monacoLanguages";
import { useThemeListener } from "../../hooks/useThemeListener";
import { getEditorMeta } from "../../types";
import { StateView } from "../../components/shared/StateView";
import { ContextMenu } from "../../components/shared/ContextMenu";
import { BASE_EDITOR_OPTIONS } from "../../lib/monacoConfig";
import { initExtMap, languageIdFromExt } from "../../lib/extToLang";
import { getFileExtension, isImagePath } from "../../lib/pathUtils";
import type { TabContentProps } from "../types";
import { defineKosmosTheme } from "./monacoTheme";
import { registerEditorOpener } from "./editorOpener";
import { editorCache } from "./editorCache";
import { attachMiddleClickPasteGuard } from "./middleClickGuard";
import { attachEditorKeybindings } from "./editorKeybindings";
import { buildContextMenuItems } from "./buildContextMenuItems";
import { createModelContentChangeHandler } from "./lspSyncHandler";
import { useDiffDecorations } from "./hooks/useDiffDecorations";
import { useGitBlame } from "./hooks/useGitBlame";
import { useAiGutter } from "./hooks/useAiGutter";
import { useExternalFileChangeListener } from "./hooks/useExternalFileChangeListener";
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

  useDiffDecorations({ editorRef, editorReady, workspace, filePath });

  const { clearBlameWidget, scheduleBlameUpdate } = useGitBlame({
    editorRef,
    editorReady,
    workspace,
    filePath,
    editorFontSize,
  });

  const { scheduleAiGutterRefresh, handleGlyphMarginClick } = useAiGutter({
    editorRef,
    monacoRef,
    editorReady,
    workspace,
    fileUri,
    filePath,
    lspLanguageRef,
  });

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

  useEffect(() => {
    editorRef.current?.updateOptions({ fontSize: editorFontSize });
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

      scheduleAiGutterRefresh();

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

  const clearTabDirty = useCallback(() => setTabDirty(tab.id, false), [setTabDirty, tab.id]);
  useExternalFileChangeListener({
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
    onContentReplaced: setContent,
    clearDirty: clearTabDirty,
  });

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

    attachEditorKeybindings(instance, monaco, {
      save: () => saveFileRef.current(),
      zoomIn: zoomEditorIn,
      zoomOut: zoomEditorOut,
      resetZoom: resetEditorZoom,
    });

    const editorDom = instance.getDomNode();
    if (editorDom) {
      middleDragCleanupRef.current = attachMiddleClickPasteGuard(editorDom);
    }

    instance.onDidChangeCursorPosition((e) => {
      scheduleBlameUpdate(e.position.lineNumber);
    });

    changeDisposableRef.current = instance.onDidChangeModelContent(
      createModelContentChangeHandler({
        instance,
        tabId: tab.id,
        workspace,
        fileUri,
        contentRef,
        isExternalUpdateRef,
        savedVersionIdRef,
        versionRef,
        pendingChangesRef,
        debounceTimerRef,
        lspLanguageRef,
        onContentChanged: scheduleAiGutterRefresh,
      }),
    );

    instance.onMouseDown(handleGlyphMarginClick);
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

  const contextMenuItems = buildContextMenuItems(editorRef.current);

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
            glyphMargin: true,
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
