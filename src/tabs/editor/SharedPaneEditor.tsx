import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useMonaco } from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import { useEditorStore } from "../../store/editor.store";
import { useLspStore } from "../../store/lsp.store";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { useThemeListener } from "../../hooks/useThemeListener";
import { ContextMenu } from "../../components/shared/ContextMenu";
import { BASE_EDITOR_OPTIONS } from "../../lib/monacoConfig";
import { getFileExtension, isImagePath } from "../../lib/pathUtils";
import { getEditorMeta, type Tab } from "../../types";
import { defineKosmosTheme } from "./monacoTheme";
import { editorCache } from "./editorCache";
import { attachMiddleClickPasteGuard } from "./middleClickGuard";
import { attachEditorKeybindings } from "./editorKeybindings";
import { buildContextMenuItems } from "./buildContextMenuItems";
import { initMonaco } from "./monacoInit";
import {
  getModelEntry,
  handleAiGlyphClick,
  onRegistryEvent,
  refreshDiffDecorations,
  setModelSavedVersion,
} from "./modelRegistry";
import { saveViewportState, getViewportState } from "./viewportState";
import { useEditorTabUiStore } from "./editorTabUiStore";

// Monaco EditorOption numeric ids: fontSize = 61, lineHeight = 75.
const FONT_SIZE_OPT = 61;
const LINE_HEIGHT_OPT = 75;
const BLAME_DEBOUNCE_MS = 500;

/**
 * One Monaco editor per pane. Binds to the active editor-tab's model from the registry
 * and swaps models (instead of creating new editors) when the user changes tabs.
 *
 * Concerns that depend on the live editor instance live here (cursor listeners, blame
 * widget, keybindings, context menu, focus). File-level concerns — dirty tracking,
 * decorations, LSP sync — live in the registry and attach to the model directly.
 */
export function SharedPaneEditor({
  paneId,
  activeTab,
  isPaneFocused,
}: {
  paneId: string;
  activeTab: Tab | null;
  isPaneFocused: boolean;
}) {
  const monaco = useMonaco();
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const [editorReady, setEditorReady] = useState(false);
  const cleanupRef = useRef<(() => void)[]>([]);
  const activeFilePathRef = useRef<string | null>(null);
  const previousTabIdRef = useRef<string | null>(null);
  const blameWidgetRef = useRef<editor.IContentWidget | null>(null);
  const blameTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const blameLineRef = useRef<number>(0);

  const workspace = useActiveWorkspace();
  const editorFontSize = useEditorStore((s) => s.editorFontSize);
  const zoomEditorIn = useEditorStore((s) => s.zoomEditorIn);
  const zoomEditorOut = useEditorStore((s) => s.zoomEditorOut);
  const resetEditorZoom = useEditorStore((s) => s.resetEditorZoom);
  const setLastClickedEditor = useEditorStore((s) => s.setLastClickedEditor);

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const svgCodeMode = useEditorTabUiStore((s) => s.svgCodeMode);

  // Compute whether the shared editor should be visible for this pane's active tab.
  const activeEditorFilePath = (() => {
    if (!activeTab) return null;
    if (activeTab.type !== "editor") return null;
    const meta = getEditorMeta(activeTab);
    if (!meta) return null;
    const fp = meta.filePath;
    if (isImagePath(fp)) {
      // SVGs can toggle into source-edit mode; otherwise images use the image viewer.
      const isSvg = getFileExtension(fp) === "svg";
      if (!(isSvg && svgCodeMode.has(activeTab.id))) return null;
    }
    return fp;
  })();

  activeFilePathRef.current = activeEditorFilePath;

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
      const fp = activeFilePathRef.current;
      const ws = workspace;
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

        // Cursor may have moved during the await; also bail if model swapped.
        if (blameLineRef.current !== lineNumber) return;
        if (activeFilePathRef.current !== fp) return;

        clearBlameWidget();
        if (blame) {
          const model = ed.getModel();
          if (!model) return;
          const endCol = model.getLineMaxColumn(lineNumber);
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
          blameWidgetRef.current = widget;
          ed.addContentWidget(widget);
        }
      } catch {
        clearBlameWidget();
      }
    },
    [workspace, clearBlameWidget],
  );

  const scheduleBlameUpdate = useCallback(
    (line: number) => {
      if (blameTimerRef.current != null) clearTimeout(blameTimerRef.current);
      clearBlameWidget();
      blameLineRef.current = 0;
      blameTimerRef.current = setTimeout(() => {
        blameTimerRef.current = null;
        updateBlame(line);
      }, BLAME_DEBOUNCE_MS);
    },
    [clearBlameWidget, updateBlame],
  );

  // Save baseline logic. Looks up the active-file model and writes its contents out.
  const saveActiveFile = useCallback(async () => {
    const ed = editorRef.current;
    const fp = activeFilePathRef.current;
    if (!ed || !fp) return;
    const model = ed.getModel();
    if (!model) return;
    const savingVersionId = model.getAlternativeVersionId();
    const savingContent = model.getValue();
    try {
      await invoke("write_file", { path: fp, content: savingContent });
      setModelSavedVersion(fp, savingVersionId);

      const entry = getModelEntry(fp);
      if (entry?.lspBinding) {
        const state = useLspStore.getState();
        const client = state.getClient(
          entry.lspBinding.workspacePath,
          entry.lspBinding.lspLanguage,
        );
        client?.didSave(entry.lspBinding.fileUri, savingContent);
        for (const companion of state.getCompanionClients(
          entry.lspBinding.workspacePath,
          entry.lspBinding.lspLanguage,
        )) {
          companion.didSave(entry.lspBinding.fileUri, savingContent);
        }
      }
    } catch (e) {
      console.error("save failed:", e);
    }
  }, []);

  const saveActiveFileRef = useRef(saveActiveFile);
  saveActiveFileRef.current = saveActiveFile;

  // Create the editor once monaco is ready.
  useEffect(() => {
    if (!monaco) return;
    const host = hostRef.current;
    if (!host) return;

    initMonaco(monaco);

    const instance = monaco.editor.create(host, {
      ...BASE_EDITOR_OPTIONS,
      theme: "kosmos",
      fontSize: useEditorStore.getState().editorFontSize,
      renderLineHighlight: "line",
      cursorBlinking: "smooth",
      cursorSmoothCaretAnimation: "on",
      bracketPairColorization: { enabled: true },
      guides: { indentation: true, bracketPairs: false },
      hover: { above: false },
      glyphMargin: true,
      // Start with no model; setModel fires once active tab resolves.
      model: null,
    });
    editorRef.current = instance;

    // Re-measure after webfonts load to avoid cursor-glyph offset.
    document.fonts.ready.then(() => {
      if (editorRef.current === instance) monaco.editor.remeasureFonts();
    });

    const disposables: { dispose(): void }[] = [];

    disposables.push(
      instance.onDidChangeCursorPosition((e) => {
        scheduleBlameUpdate(e.position.lineNumber);
      }),
    );

    disposables.push(
      instance.onDidFocusEditorWidget(() => {
        const fp = activeFilePathRef.current;
        if (fp) setLastClickedEditor(fp);
      }),
    );

    disposables.push(
      instance.onMouseDown((e) => {
        const fp = activeFilePathRef.current;
        if (!fp) return;
        if (e.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return;
        const line = e.target.position?.lineNumber;
        if (!line) return;
        const entry = getModelEntry(fp);
        if (entry?.aiFunctions.has(line)) {
          e.event.preventDefault();
          e.event.stopPropagation();
          handleAiGlyphClick(fp, line, monaco);
        }
      }),
    );

    disposables.push(
      attachEditorKeybindings(instance, monaco, {
        save: () => saveActiveFileRef.current(),
        zoomIn: zoomEditorIn,
        zoomOut: zoomEditorOut,
        resetZoom: resetEditorZoom,
      }),
    );

    const editorDom = instance.getDomNode();
    const middleCleanup = editorDom ? attachMiddleClickPasteGuard(editorDom) : null;

    cleanupRef.current = [
      ...disposables.map((d) => () => d.dispose()),
      ...(middleCleanup ? [middleCleanup] : []),
    ];

    setEditorReady(true);

    return () => {
      for (const fn of cleanupRef.current) fn();
      cleanupRef.current = [];
      if (blameTimerRef.current != null) clearTimeout(blameTimerRef.current);
      clearBlameWidget();

      // Snapshot view state for the currently-bound tab before disposing.
      const prevTabId = previousTabIdRef.current;
      if (prevTabId) saveViewportState(prevTabId, instance.saveViewState());

      instance.dispose();
      editorRef.current = null;
      setEditorReady(false);
    };
    // zoom/resetEditor callbacks are stable; scheduleBlameUpdate / clearBlameWidget depend on workspace through ref.
    // Intentionally run this effect once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [monaco]);

  useEffect(() => {
    editorRef.current?.updateOptions({ fontSize: editorFontSize });
  }, [editorFontSize]);

  // Theme updates apply to the shared singleton theme.
  const handleThemeChanged = useCallback(() => {
    if (!monaco) return;
    defineKosmosTheme(monaco);
    monaco.editor.setTheme("kosmos");
  }, [monaco]);
  useThemeListener(handleThemeChanged);

  // Bind the editor to the active tab's model. Re-runs on activeTab change AND when
  // a "model-created" event indicates the tab's file just became available.
  useEffect(() => {
    if (!editorReady) return;
    const ed = editorRef.current;
    if (!ed) return;

    const applyBinding = () => {
      const fp = activeFilePathRef.current;
      const activeId = activeTab?.id ?? null;

      // Save the outgoing tab's viewport.
      const prevTabId = previousTabIdRef.current;
      if (prevTabId && prevTabId !== activeId) {
        const state = ed.saveViewState();
        if (state) saveViewportState(prevTabId, state);
      }

      if (!fp || !activeId) {
        ed.setModel(null);
        clearBlameWidget();
        blameLineRef.current = 0;
        previousTabIdRef.current = activeId;
        return;
      }

      const entry = getModelEntry(fp);
      if (!entry) return; // Wait for the model-created event.

      if (ed.getModel() !== entry.model) {
        clearBlameWidget();
        blameLineRef.current = 0;
        ed.setModel(entry.model);
      }

      const state = getViewportState(activeId);
      if (state) ed.restoreViewState(state);

      previousTabIdRef.current = activeId;

      // Refresh the editorCache entry for this file so TopMenus / save-all can find us.
      const existing = editorCache.get(fp);
      editorCache.set(fp, {
        instance: ed,
        pendingReveal: existing?.pendingReveal,
        save: () => saveActiveFileRef.current(),
      });
      if (existing?.pendingReveal) {
        const pos = existing.pendingReveal;
        setTimeout(() => {
          const current = editorCache.get(fp);
          if (!current || current.pendingReveal !== pos) return;
          if (!editorRef.current) return;
          if (activeFilePathRef.current !== fp) return;
          editorRef.current.setPosition(pos);
          editorRef.current.revealPositionInCenter(pos);
          editorCache.set(fp, { ...current, pendingReveal: undefined });
        }, 50);
      }

      // Kick off diff + blame refresh for the newly-bound file.
      void refreshDiffDecorations(fp, workspace);
      const cursor = ed.getPosition();
      if (cursor) scheduleBlameUpdate(cursor.lineNumber);
    };

    applyBinding();

    // Watch for the model to appear if EditorTab hasn't acquired it yet.
    const unsub = onRegistryEvent((e) => {
      if (e.type === "model-created" && e.filePath === activeFilePathRef.current) {
        applyBinding();
      }
    });
    return unsub;
  }, [editorReady, activeTab, workspace, clearBlameWidget, scheduleBlameUpdate]);

  // When our pane's active tab becomes active AND the pane is the focused one, bring
  // focus to the editor. Skip if focus already lives inside, to avoid stealing from
  // popups (completions, go-to-def, etc.) or from other panes.
  useEffect(() => {
    if (!isPaneFocused) return;
    if (!activeEditorFilePath) return;
    const raf = requestAnimationFrame(() => {
      const ed = editorRef.current;
      if (!ed) return;
      const dom = ed.getDomNode();
      const active = document.activeElement;
      if (dom && active && dom.contains(active)) return;
      ed.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [isPaneFocused, activeEditorFilePath]);

  // Track lastClickedEditor when this editor has focus, so TopMenus actions target it.
  useEffect(() => {
    if (!isPaneFocused || !activeEditorFilePath) return;
    setLastClickedEditor(activeEditorFilePath);
  }, [isPaneFocused, activeEditorFilePath, setLastClickedEditor]);

  // Re-apply diff on workspace change while the same file is bound.
  useEffect(() => {
    if (!activeEditorFilePath) return;
    void refreshDiffDecorations(activeEditorFilePath, workspace);
  }, [activeEditorFilePath, workspace]);

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  const contextMenuItems = buildContextMenuItems(editorRef.current);

  // Visibility: the host div is hidden when no editor tab is active so tab containers
  // (file tree, terminal, image viewer) underneath are interactable.
  const visible = activeEditorFilePath !== null;

  return (
    <div
      data-pane-editor={paneId}
      className="absolute inset-0"
      style={{
        pointerEvents: visible ? "auto" : "none",
        visibility: visible ? "visible" : "hidden",
      }}
      onContextMenu={handleContextMenu}
    >
      <div ref={hostRef} className="h-full w-full" />
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
