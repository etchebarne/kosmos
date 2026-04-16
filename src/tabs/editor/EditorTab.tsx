import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
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
import { findLeaf } from "../../lib/pane-tree";
import { setupMonacoLanguages, resolveModelLanguage } from "../../lib/lsp/monaco-languages";
import { useThemeListener } from "../../hooks/use-theme-listener";
import { getEditorMeta } from "../../types";
import { StateView } from "../../components/shared/StateView";
import { ContextMenu, type ContextMenuItem } from "../../components/shared/ContextMenu";
import { BASE_EDITOR_OPTIONS } from "../../lib/monaco-config";
import { initExtMap, languageIdFromExt } from "../../lib/ext-to-lang";
import { normalizePath, getFileExtension } from "../../lib/path-utils";
import type { TabContentProps } from "../types";
import { defineKosmosTheme } from "./monaco-theme";
import { registerEditorOpener } from "./editor-opener";
import { editorCache } from "./editor-cache";
import { parseDiffChanges, buildDiffDecorations } from "./diff-decorations";

// ── Language detection from file extension (for early LSP start) ──

function languageIdFromPath(filePath: string): string {
  const ext = getFileExtension(filePath);
  return (ext && languageIdFromExt(ext)) ?? "plaintext";
}

export function EditorTab({ tab, paneId }: TabContentProps) {
  const filePath = getEditorMeta(tab)?.filePath;
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const dirty = useLayoutStore((s) => s.dirtyTabs.has(tab.id));
  const setTabDirty = useLayoutStore((s) => s.setTabDirty);
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const contentRef = useRef<string | null>(null);
  // Monaco's alternativeVersionId snapshot at the last saved/loaded baseline.
  // Dirty is derived by comparing it to the model's current id: if the user
  // undoes back to this baseline, dirty clears automatically (same approach
  // VS Code uses). Survives stray change events better than a plain flag.
  const savedVersionIdRef = useRef(0);
  const versionRef = useRef(0);
  const changeDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const pendingChangesRef = useRef<TextDocumentContentChangeEvent[]>([]);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [editorReady, setEditorReady] = useState(false);
  const lspOpenedRef = useRef(false);
  const diffDecorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null);
  const blameWidgetRef = useRef<editor.IContentWidget | null>(null);
  const blameTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const blameLineRef = useRef<number>(0);

  const editorFontSize = useEditorStore((s) => s.editorFontSize);
  const zoomEditorIn = useEditorStore((s) => s.zoomEditorIn);
  const zoomEditorOut = useEditorStore((s) => s.zoomEditorOut);
  const resetEditorZoom = useEditorStore((s) => s.resetEditorZoom);

  const workspace = useActiveWorkspace();
  const startServer = useLspStore((s) => s.startServer);
  const startCompanions = useLspStore((s) => s.startCompanions);
  const getClient = useLspStore((s) => s.getClient);
  const getCompanionClients = useLspStore((s) => s.getCompanionClients);
  const lspLanguageRef = useRef<string>("plaintext");

  const fileUri = filePath ? pathToFileUri(filePath) : null;

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const isExternalUpdateRef = useRef(false);

  // Refs to keep cleanup closure in sync with latest values
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const fileUriRef = useRef(fileUri);
  fileUriRef.current = fileUri;
  const filePathRef = useRef(filePath);
  filePathRef.current = filePath;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  const loadFile = useCallback(async () => {
    if (!filePath) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<string>("read_file", { path: filePath });
      setContent(result);
      contentRef.current = result;
      // If the editor already exists (rare: loadFile re-runs after mount),
      // rebaseline the saved version id. The common path is initial load,
      // where the editor mounts after this and captures its id in onMount.
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
    // Snapshot the version id we're about to persist. We capture *before*
    // the async write so edits made during the write don't get baselined
    // as saved.
    const model = editorRef.current?.getModel();
    const savingVersionId = model?.getAlternativeVersionId() ?? 0;
    const savingContent = contentRef.current;
    try {
      await invoke("write_file", { path: filePath, content: savingContent });
      savedVersionIdRef.current = savingVersionId;
      // Re-evaluate dirty against the (possibly newer) current version id.
      const currentVid = editorRef.current?.getModel()?.getAlternativeVersionId() ?? 0;
      setTabDirty(tab.id, currentVid !== savingVersionId);

      // Notify LSP of save
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

  // Keep a ref to saveFile so the once-registered onKeyDown handler always
  // calls the latest closure (picks up workspace/fileUri after they load).
  const saveFileRef = useRef(saveFile);
  saveFileRef.current = saveFile;

  // ── Git diff gutter decorations ──

  const refreshDiffDecorations = useCallback(async () => {
    const ed = editorRef.current;
    const ws = workspaceRef.current;
    const fp = filePathRef.current;
    if (!ed || !ws || !fp) return;

    // Convert absolute path to workspace-relative
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
      // File may not be tracked by git — clear any stale decorations
      diffDecorationsRef.current?.clear();
    }
  }, []);

  // Refresh diff decorations when editor + workspace are ready
  useEffect(() => {
    if (editorReady && workspace) refreshDiffDecorations();
  }, [editorReady, workspace, refreshDiffDecorations]);

  // Refresh diff decorations when git state changes
  useEffect(() => {
    const unlisten = listen("git-changed", () => {
      refreshDiffDecorations();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refreshDiffDecorations]);

  // ── Inline git blame (GitLens-style) ──

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

      // If the line hasn't changed, skip
      if (lineNumber === blameLineRef.current) return;
      blameLineRef.current = lineNumber;

      const relative = fp.startsWith(ws.path + "/") ? fp.slice(ws.path.length + 1) : fp;

      try {
        const blame = await invoke<string | null>("git_blame_line", {
          path: ws.path,
          file: relative,
          line: lineNumber,
        });

        // Cursor may have moved while we were fetching
        if (blameLineRef.current !== lineNumber) return;

        clearBlameWidget();
        if (blame) {
          const model = ed.getModel();
          const endCol = model ? model.getLineMaxColumn(lineNumber) : 1;
          const domNode = document.createElement("div");
          domNode.className = "git-blame-inline";
          domNode.style.fontSize = `${ed.getOption(/* EditorOption.fontSize */ 61) * 0.85}px`;
          domNode.style.lineHeight = `${ed.getOption(/* EditorOption.lineHeight */ 75)}px`;
          domNode.textContent = blame;
          const widget: editor.IContentWidget = {
            getId: () => "git-blame-widget",
            getDomNode: () => domNode,
            getPosition: () => ({
              position: { lineNumber, column: endCol },
              preference: [0], // EXACT
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

  // Trigger initial blame when editor + workspace are ready
  useEffect(() => {
    if (!editorReady || !workspace) return;
    const pos = editorRef.current?.getPosition();
    if (pos) updateBlame(pos.lineNumber);
  }, [editorReady, workspace, updateBlame]);

  // Sync font size from store to the editor instance
  useEffect(() => {
    editorRef.current?.updateOptions({ fontSize: editorFontSize });
    // Update existing blame widget font size to match
    const widget = blameWidgetRef.current;
    if (widget) {
      const ed = editorRef.current;
      const fs = ed?.getOption(/* EditorOption.fontSize */ 61);
      const lh = ed?.getOption(/* EditorOption.lineHeight */ 75);
      if (fs) widget.getDomNode().style.fontSize = `${fs * 0.85}px`;
      if (lh) widget.getDomNode().style.lineHeight = `${lh}px`;
    }
  }, [editorFontSize]);

  // Re-apply Monaco theme when the app theme changes
  const handleThemeChanged = useCallback(() => {
    const monaco = monacoRef.current;
    if (!monaco) return;
    defineKosmosTheme(monaco);
    monaco.editor.setTheme("kosmos");
  }, []);
  useThemeListener(handleThemeChanged);

  // Start LSP when both editor and workspace are ready.
  // Handles the case where workspace loads after the editor mounts (release builds)
  // and the case where the editor mounts after workspace is already available.
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

      // Start companion servers (e.g. tailwindcss) and send didOpen to them
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

  // Cleanup on unmount: flush pending changes, didClose, change listener, editor instance.
  // Uses refs to always access the latest workspace/fileUri/filePath values.
  useEffect(() => {
    return () => {
      const ws = workspaceRef.current;
      const uri = fileUriRef.current;
      const fp = filePathRef.current;

      // Clear debounce timer and flush any pending changes before closing
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

      if (blameTimerRef.current != null) {
        clearTimeout(blameTimerRef.current);
        blameTimerRef.current = null;
      }
      clearBlameWidget();

      lspOpenedRef.current = false;
      changeDisposableRef.current?.dispose();
      useLayoutStore.getState().setTabDirty(tab.id, false);
      if (fp) editorCache.delete(fp);
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

  // Refocus Monaco when this tab becomes active in its pane. Tabs stay
  // mounted across switches (portaled into inert containers), so DOM focus
  // is dropped by the `inert` attribute when leaving. Without this refocus,
  // Ctrl+S — bound via Monaco's addCommand, which only fires when the
  // editor has focus — silently no-ops on return, and the dirty dot looks
  // stuck. Ctrl+S intentionally only acts on the focused editor so that
  // with split panes / multiple visible editors there's no ambiguity about
  // which file gets saved.
  //
  // Run in rAF so the PanePortalContext's useLayoutEffect has already
  // removed `inert`; calling focus() on an inert element silently fails.
  // Only take focus if it's currently outside this editor — avoids
  // stealing focus from a widget the user intentionally clicked inside.
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

  // Reload editor content when the file is modified externally and the editor is clean.
  // Uses refs for workspace/fileUri/getClient to keep the listener stable across store updates.
  useEffect(() => {
    if (!filePath) return;

    const unlisten = listen<string[]>("file-content-changed", async (event) => {
      const changedFiles = event.payload;
      // Normalize both paths for comparison (backslash-insensitive)
      const normFilePath = normalizePath(filePath);
      if (!changedFiles.some((f) => normalizePath(f) === normFilePath)) return;

      // Don't reload if the user has unsaved edits
      if (dirtyRef.current) return;

      try {
        const newContent = await invoke<string>("read_file", { path: filePath });
        // Skip if content is identical (e.g. triggered by our own save)
        if (newContent === contentRef.current) return;

        contentRef.current = newContent;
        const ed = editorRef.current;
        if (ed) {
          isExternalUpdateRef.current = true;
          ed.setValue(newContent);
          isExternalUpdateRef.current = false;
          // Content now matches disk again — rebaseline the saved version id
          // so the dirty dot stays off.
          const model = ed.getModel();
          if (model) savedVersionIdRef.current = model.getAlternativeVersionId();
          setTabDirty(tab.id, false);
        } else {
          setContent(newContent);
        }

        // Notify LSP of the updated content
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
        // File may have been deleted — ignore
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [filePath]);

  function handleEditorDidMount(instance: editor.IStandaloneCodeEditor, monaco: Monaco) {
    editorRef.current = instance;
    monacoRef.current = monaco;

    // Remeasure fonts once loaded — fixes cursor offset when web fonts swap in
    document.fonts.ready.then(() => {
      if (editorRef.current) monaco.editor.remeasureFonts();
    });

    // Resolve the model to the most specific registered language (e.g.
    // "typescriptreact" instead of "typescript" for .tsx files).
    const model = instance.getModel();
    if (model) resolveModelLanguage(monaco, model);
    lspLanguageRef.current = model?.getLanguageId() ?? "plaintext";

    // Baseline the saved version id to the model's current id. Editor just
    // mounted with on-disk content, so this is our "clean" reference point.
    savedVersionIdRef.current = model?.getAlternativeVersionId() ?? 0;
    // Defensive: make sure the dirty flag is cleared on mount.
    useLayoutStore.getState().setTabDirty(tab.id, false);

    // Signal that the editor is mounted — the LSP useEffect will handle
    // starting the server and sending didOpen when all conditions are met.
    setEditorReady(true);

    // Grab keyboard focus so the user can type immediately
    instance.focus();

    // Register editor instance for cross-file navigation
    if (filePath) {
      const cached = editorCache.get(filePath);
      const pendingReveal = cached?.pendingReveal;
      editorCache.set(filePath, { instance, pendingReveal: undefined });
      if (pendingReveal) {
        // Defer reveal — @monaco-editor/react toggles the container from
        // display:none to display:block after onMount, triggering a
        // ResizeObserver → layout() that resets scroll. setTimeout runs
        // after the ResizeObserver callback settles.
        setTimeout(() => {
          instance.setPosition(pendingReveal);
          instance.revealPositionInCenter(pendingReveal);
        }, 50);
      }
    }

    // Bind Ctrl+S / zoom keys via onKeyDown (per-editor) rather than
    // addCommand. Monaco's addCommand uses a single global keybinding
    // registry — if two editors are mounted and both addCommand(Ctrl+S),
    // the second registration overrides the first, so only one editor's
    // handler ever fires. onKeyDown is a plain per-instance event, scoped
    // to this editor's focus, with no cross-instance interference.
    //
    // Refs keep the handler reading the latest store actions / saveFile
    // closure without rebinding on every render.
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

    // Debounced inline blame on cursor move
    instance.onDidChangeCursorPosition((e) => {
      if (blameTimerRef.current != null) clearTimeout(blameTimerRef.current);
      // Clear immediately when moving away
      clearBlameWidget();
      blameLineRef.current = 0;
      blameTimerRef.current = setTimeout(() => {
        blameTimerRef.current = null;
        updateBlame(e.position.lineNumber);
      }, 500);
    });

    // Listen for content changes and debounce LSP didChange notifications.
    // Sending on every keystroke floods the server; batching with a short
    // delay reduces load while keeping diagnostics responsive.
    const DIDCHANGE_DEBOUNCE_MS = 200;

    changeDisposableRef.current = instance.onDidChangeModelContent((e) => {
      // Update local state immediately (not debounced)
      contentRef.current = instance.getValue();
      // Skip dirty flag for programmatic reloads from external file changes
      if (isExternalUpdateRef.current) return;

      // Derive dirty from Monaco's alternativeVersionId — self-correcting
      // on undo, resilient to stray change events. Read the current store
      // state (not the stale `dirty` render-time value) so we only call
      // setTabDirty when the flag actually needs to flip.
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

      // Check if server wants full or incremental sync
      const syncKind =
        typeof client.capabilities?.textDocumentSync === "object"
          ? client.capabilities.textDocumentSync.change
          : client.capabilities?.textDocumentSync;

      // Accumulate changes for the debounced send
      if (syncKind === TextDocumentSyncKind.Full) {
        // For full sync, only the latest snapshot matters
        pendingChangesRef.current = [{ text: instance.getValue() }];
      } else {
        // For incremental sync, accumulate all changes in order
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

      // Debounce the actual send
      if (debounceTimerRef.current != null) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        debounceTimerRef.current = null;
        if (pendingChangesRef.current.length === 0) return;
        const currentClient = getClient(workspace.path, lspLanguageRef.current);
        currentClient?.didChange(fileUri, versionRef.current, pendingChangesRef.current);
        // Mirror changes to companion servers (e.g. tailwindcss)
        for (const companion of getCompanionClients(workspace.path, lspLanguageRef.current)) {
          companion.didChange(fileUri, versionRef.current, pendingChangesRef.current);
        }
        pendingChangesRef.current = [];
      }, DIDCHANGE_DEBOUNCE_MS);
    });
  }

  function handleBeforeMount(monaco: Monaco) {
    monacoRef.current = monaco;
    defineKosmosTheme(monaco);
    setupMonacoLanguages(monaco);
    initExtMap(monaco);
    registerEditorOpener(monaco);

    // Eagerly start the LSP server while Monaco finishes mounting the editor.
    // This overlaps server spawn + initialize with editor DOM setup, so
    // providers are ready sooner. The onMount handler will await the same
    // shared promise and send didOpen once it resolves.
    if (workspace && filePath) {
      const lang = languageIdFromPath(filePath);
      lspLanguageRef.current = lang;
      startServer(workspace.path, lang, filePath, monaco);
    }

    // Disable Monaco's built-in TS/JS diagnostics unconditionally.
    // They run in-browser without tsconfig/node_modules so they always
    // produce false positives. Real diagnostics come from the LSP server.
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
