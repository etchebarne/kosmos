import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Code, Image as ImageIcon } from "@phosphor-icons/react";
import { useMonaco } from "@monaco-editor/react";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { useLspStore } from "../../store/lsp.store";
import { pathToFileUri } from "../../lib/lsp/uri";
import { useIsTabActive } from "../../hooks/useIsTabActive";
import { useWorkspaceWatch } from "../../hooks/useWorkspaceWatch";
import { getEditorMeta } from "../../types";
import { StateView } from "../../components/shared/StateView";
import { languageIdFromExt } from "../../lib/extToLang";
import { getFileExtension, isImagePath } from "../../lib/pathUtils";
import type { TabContentProps } from "../types";
import { editorCache } from "./editorCache";
import { initMonaco } from "./monacoInit";
import { acquireModel, openLspCompanionsForModel, openLspForModel } from "./modelRegistry";
import { clearViewportState } from "./viewportState";
import { useEditorTabUiStore } from "./editorTabUiStore";
import { ImageViewer } from "./ImageViewer";

function languageIdFromPath(filePath: string): string {
  const ext = getFileExtension(filePath);
  return (ext && languageIdFromExt(ext)) ?? "plaintext";
}

export function EditorTab({ tab, paneId }: TabContentProps) {
  const filePath = getEditorMeta(tab)?.filePath;
  const workspace = useActiveWorkspace();
  const isActiveTab = useIsTabActive(paneId, tab.id);
  const isImage = filePath ? isImagePath(filePath) : false;
  const isSvg = filePath ? getFileExtension(filePath) === "svg" : false;
  const svgShowCode = useEditorTabUiStore((s) => (isSvg ? s.svgCodeMode.has(tab.id) : false));
  const setSvgCodeMode = useEditorTabUiStore((s) => s.setSvgCodeMode);

  useWorkspaceWatch(workspace?.path ?? null, isActiveTab);

  // Non-image editor tabs always hand off rendering to the pane's SharedPaneEditor —
  // model acquisition + LSP live in EditorTabContent, which renders no UI of its own.
  if (!isImage) {
    return <EditorTabContent tab={tab} filePath={filePath} />;
  }

  const handleShowPreview = async () => {
    // Flush any unsaved edits so the preview reflects them.
    if (filePath) await editorCache.get(filePath)?.save?.();
    setSvgCodeMode(tab.id, false);
  };

  return (
    <div className="relative h-full">
      {svgShowCode ? (
        <EditorTabContent tab={tab} filePath={filePath} />
      ) : (
        <ImageViewer filePath={filePath!} />
      )}
      {isSvg && (
        <button
          type="button"
          onClick={svgShowCode ? handleShowPreview : () => setSvgCodeMode(tab.id, true)}
          className="absolute top-2 right-2 z-10 flex items-center gap-1.5 h-7 px-2.5 text-[11px] font-medium bg-[var(--color-bg-surface)] text-[var(--color-text-secondary)] border border-[var(--color-border-secondary)] hover:border-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors rounded cursor-pointer"
          title={svgShowCode ? "Show image preview" : "Edit SVG source"}
        >
          {svgShowCode ? <ImageIcon size={12} /> : <Code size={12} />}
          {svgShowCode ? "Preview" : "Edit code"}
        </button>
      )}
    </div>
  );
}

/**
 * Headless shell for editor-type tabs. Loads the file, acquires a model in the
 * registry, opens the LSP document, and releases on unmount. The pane's
 * SharedPaneEditor overlays the tabpanel and binds to whichever tab is active.
 */
function EditorTabContent({
  tab,
  filePath,
}: Omit<TabContentProps, "paneId"> & { filePath: string | undefined }) {
  const monaco = useMonaco();
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const releaseModelRef = useRef<(() => void) | null>(null);
  const lspLanguageRef = useRef<string>("plaintext");
  const lspOpenedRef = useRef(false);

  const workspace = useActiveWorkspace();
  const startServer = useLspStore((s) => s.startServer);
  const startCompanions = useLspStore((s) => s.startCompanions);

  // Eagerly kick off the LSP server so providers are ready by the time the user
  // interacts with the file. Safe even before the model exists.
  useEffect(() => {
    if (!monaco || !workspace || !filePath) return;
    initMonaco(monaco);
    const lang = languageIdFromPath(filePath);
    lspLanguageRef.current = lang;
    startServer(workspace.path, lang, filePath, monaco);
  }, [monaco, workspace, filePath, startServer]);

  // Load the file and acquire a model in the registry.
  useEffect(() => {
    if (!monaco || !filePath) return;
    let cancelled = false;

    (async () => {
      try {
        const content = await invoke<string>("read_file", { path: filePath });
        if (cancelled) return;
        initMonaco(monaco);
        const lang = languageIdFromPath(filePath);
        const { release, entry } = acquireModel({
          tabId: tab.id,
          filePath,
          monaco,
          initialContent: content,
          languageId: lang,
        });
        if (cancelled) {
          release();
          return;
        }
        releaseModelRef.current = release;
        // Registry may have narrowed to a more specific language (e.g. typescriptreact).
        lspLanguageRef.current = entry.model.getLanguageId();
        setReady(true);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [monaco, filePath, tab.id]);

  // Open the LSP document once the model is acquired and the server is ready.
  useEffect(() => {
    if (!ready || !workspace || !filePath) return;
    if (lspOpenedRef.current) return;

    const fileUri = pathToFileUri(filePath);
    const lang = lspLanguageRef.current;
    let cancelled = false;

    startServer(workspace.path, lang, filePath, null).then((client) => {
      if (cancelled || !client) return;
      lspOpenedRef.current = true;
      openLspForModel({
        filePath,
        workspacePath: workspace.path,
        fileUri,
        lspLanguage: lang,
      });

      startCompanions(workspace.path, lang, filePath, null).then(() => {
        if (cancelled) return;
        openLspCompanionsForModel(filePath);
      });
    });

    return () => {
      cancelled = true;
    };
  }, [ready, workspace, filePath, startServer, startCompanions]);

  // Release the model on unmount. Last release disposes the model + sends didClose.
  useEffect(() => {
    const tabId = tab.id;
    return () => {
      releaseModelRef.current?.();
      releaseModelRef.current = null;
      lspOpenedRef.current = false;
      clearViewportState(tabId);
      if (filePath) editorCache.delete(filePath);
    };
  }, [tab.id, filePath]);

  if (!filePath) {
    return <StateView message="No file path" />;
  }

  if (error) {
    return <StateView message={error} variant="error" />;
  }

  // Invisible placeholder so the tab container has some layout, but visually the
  // pane's SharedPaneEditor paints everything when this tab is active.
  return <div className="h-full w-full" />;
}
