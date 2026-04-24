import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import MonacoEditor, { type Monaco } from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import { defineKosmosTheme } from "../editor/monacoTheme";
import { useEditorStore } from "../../store/editor.store";
import { setupMonacoLanguages, resolveModelLanguage } from "../../lib/lsp/monacoLanguages";
import { pathToFileUri } from "../../lib/lsp/uri";
import { BASE_EDITOR_OPTIONS } from "../../lib/monacoConfig";

interface FilePreviewProps {
  filePath: string;
  matchLine: number;
  query: string;
}

export function FilePreview({ filePath, matchLine, query }: FilePreviewProps) {
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const decorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null);
  const matchLineRef = useRef(matchLine);
  const queryRef = useRef(query);
  matchLineRef.current = matchLine;
  queryRef.current = query;
  const editorFontSize = useEditorStore((s) => s.editorFontSize);
  const [ready, setReady] = useState(false);

  function applyDecorations(ed: editor.IStandaloneCodeEditor, line: number, q: string) {
    decorationsRef.current?.clear();
    const decorations: editor.IModelDeltaDecoration[] = [
      {
        range: { startLineNumber: line, startColumn: 1, endLineNumber: line, endColumn: 1 },
        options: { isWholeLine: true, className: "search-match-line-highlight" },
      },
    ];

    if (q) {
      const model = ed.getModel();
      if (model && line <= model.getLineCount()) {
        const lineContent = model.getLineContent(line);
        const col = lineContent.toLowerCase().indexOf(q.toLowerCase());
        if (col !== -1) {
          decorations.push({
            range: {
              startLineNumber: line,
              startColumn: col + 1,
              endLineNumber: line,
              endColumn: col + 1 + q.length,
            },
            options: { inlineClassName: "search-match-text-highlight" },
          });
        }
      }
    }

    decorationsRef.current = ed.createDecorationsCollection(decorations);
    ed.revealLineInCenter(line);
  }

  useEffect(() => {
    const ed = editorRef.current;
    const monaco = monacoRef.current;
    if (!ed || !monaco || !ready) return;

    let cancelled = false;

    invoke<string>("read_file", { path: filePath })
      .then((content) => {
        if (cancelled) return;
        // Use a preview-only URI so we don't share / overwrite / dispose the registry's
        // model when this file is also open in an editor tab. Fragment is part of the
        // URI identity for Monaco's model cache but leaves the path intact so language
        // detection (by extension) still works.
        const uri = monaco.Uri.parse(pathToFileUri(filePath) + "#preview");
        let model = monaco.editor.getModel(uri);
        if (model) {
          model.setValue(content);
        } else {
          model = monaco.editor.createModel(content, undefined, uri);
        }
        ed.setModel(model);
        resolveModelLanguage(monaco, model);
        applyDecorations(ed, matchLineRef.current, queryRef.current);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [filePath, ready]);

  useEffect(() => {
    const ed = editorRef.current;
    if (!ed || !ready) return;
    applyDecorations(ed, matchLine, query);
  }, [matchLine, query, ready]);

  function handleBeforeMount(monaco: Monaco) {
    defineKosmosTheme(monaco);
    setupMonacoLanguages(monaco);
  }

  function handleMount(instance: editor.IStandaloneCodeEditor, monaco: Monaco) {
    editorRef.current = instance;
    monacoRef.current = monaco;
    setReady(true);
  }

  useEffect(() => {
    return () => {
      editorRef.current?.getModel()?.dispose();
    };
  }, []);

  return (
    <MonacoEditor
      theme="kosmos"
      beforeMount={handleBeforeMount}
      onMount={handleMount}
      options={{
        ...BASE_EDITOR_OPTIONS,
        readOnly: true,
        fontSize: editorFontSize,
        renderLineHighlight: "none",
        domReadOnly: true,
      }}
    />
  );
}
