import { useCallback, useEffect, useRef, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Monaco } from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import { useLspStore } from "../../../store/lsp.store";
import { useSettingsStore } from "../../../store/settings.store";
import { useToastStore } from "../../../store/toast.store";
import type { Workspace } from "../../../store/workspace.store";
import {
  AI_GENERATE_GLYPH_LOADING_CLASS,
  buildAiGutterDecorations,
  buildGenerationPrompt,
  buildWindowedContext,
  extractFunctions,
  stripCodeFences,
  type AiFunctionInfo,
} from "../aiGutter";

const REFRESH_DEBOUNCE_MS = 400;

interface AiGenerateResult {
  text: string;
  stderr: string;
  raw: string;
  tempPath: string;
}

/**
 * Renders a "generate function body" glyph in the Monaco glyph margin for every
 * function definition the LSP reports, and wires clicks to Claude Code (or other
 * agents). Concurrent generations are supported — each owns sticky decorations that
 * track the function's current range as the user continues editing.
 */
export function useAiGutter(opts: {
  editorRef: RefObject<editor.IStandaloneCodeEditor | null>;
  monacoRef: RefObject<Monaco | null>;
  editorReady: boolean;
  workspace: Workspace | null;
  fileUri: string | null;
  filePath: string | undefined;
  lspLanguageRef: RefObject<string>;
}): {
  scheduleAiGutterRefresh: () => void;
  handleGlyphMarginClick: (e: editor.IEditorMouseEvent) => void;
} {
  const { editorRef, monacoRef, editorReady, workspace, fileUri, filePath, lspLanguageRef } = opts;

  const aiCompletionEnabled = useSettingsStore((s) => s.values["ai.enableCompletion"] === true);
  const aiAgent = useSettingsStore((s) => (s.values["ai.agent"] as string) ?? "claude-code");
  const claudeCodeModel = useSettingsStore(
    (s) => (s.values["ai.claudeCode.model"] as string) ?? "sonnet",
  );

  // Mirror reactive values into refs for stable callbacks / event handlers.
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const fileUriRef = useRef(fileUri);
  fileUriRef.current = fileUri;
  const filePathRef = useRef(filePath);
  filePathRef.current = filePath;
  const aiCompletionEnabledRef = useRef(aiCompletionEnabled);
  aiCompletionEnabledRef.current = aiCompletionEnabled;
  const aiAgentRef = useRef(aiAgent);
  aiAgentRef.current = aiAgent;
  const claudeCodeModelRef = useRef(claudeCodeModel);
  claudeCodeModelRef.current = claudeCodeModel;

  const gutterDecorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null);
  const functionsRef = useRef<Map<number, AiFunctionInfo>>(new Map());
  // Each in-flight generation owns two sticky decorations: a single-line one on the
  // function's first line for the spinner glyph, and a full-range one used to compute
  // the replacement target. Two decorations because glyphMarginClassName paints on every
  // line the range covers, and the replacement needs the full function range.
  const inFlightRef = useRef<
    Map<
      number,
      {
        glyph: editor.IEditorDecorationsCollection;
        range: editor.IEditorDecorationsCollection;
      }
    >
  >(new Map());
  const generationIdRef = useRef(0);
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const rebuildDecorations = useCallback(() => {
    const ed = editorRef.current;
    if (!ed) return;
    // Skip rendering a normal glyph on any line already covered by a loading glyph.
    const inFlightLines = new Set<number>();
    for (const { glyph } of inFlightRef.current.values()) {
      const r = glyph.getRanges()[0];
      if (r) inFlightLines.add(r.startLineNumber);
    }
    const decorations = buildAiGutterDecorations(functionsRef.current, inFlightLines);
    gutterDecorationsRef.current?.clear();
    gutterDecorationsRef.current = ed.createDecorationsCollection(decorations);
  }, [editorRef]);

  const refreshAiGutter = useCallback(async () => {
    const ed = editorRef.current;
    const ws = workspaceRef.current;
    const uri = fileUriRef.current;
    if (!ed) return;

    if (!aiCompletionEnabledRef.current || !ws || !uri) {
      gutterDecorationsRef.current?.clear();
      gutterDecorationsRef.current = null;
      functionsRef.current = new Map();
      return;
    }

    const client = useLspStore.getState().getClient(ws.path, lspLanguageRef.current);
    if (!client) return;

    try {
      const symbols = await client.documentSymbol(uri);
      functionsRef.current = extractFunctions(symbols);
      rebuildDecorations();
    } catch {
      gutterDecorationsRef.current?.clear();
      gutterDecorationsRef.current = null;
      functionsRef.current = new Map();
    }
  }, [editorRef, lspLanguageRef, rebuildDecorations]);

  const scheduleAiGutterRefresh = useCallback(() => {
    if (refreshTimerRef.current != null) clearTimeout(refreshTimerRef.current);
    refreshTimerRef.current = setTimeout(() => {
      refreshTimerRef.current = null;
      refreshAiGutter();
    }, REFRESH_DEBOUNCE_MS);
  }, [refreshAiGutter]);

  const generateFunctionAtLine = useCallback(
    async (startLine: number) => {
      const ed = editorRef.current;
      const model = ed?.getModel();
      const monaco = monacoRef.current;
      const info = functionsRef.current.get(startLine);
      if (!ed || !model || !monaco || !info) return;

      // Re-click on an in-flight generation cancels it instead of starting a new one.
      for (const [existingGenId, { glyph }] of inFlightRef.current.entries()) {
        const r = glyph.getRanges()[0];
        if (r && r.startLineNumber === startLine) {
          invoke("ai_cancel", { cancelId: String(existingGenId) });
          return;
        }
      }

      const installedAgents = await invoke<string[]>("ai_installed_agents");
      if (installedAgents.length === 0) {
        useToastStore.getState().addToast({
          type: "warning",
          message: "No coding agent was found",
        });
        return;
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
            glyphMarginHoverMessage: { value: "Click to cancel" },
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

      const genId = ++generationIdRef.current;
      inFlightRef.current.set(genId, { glyph: glyphCollection, range: rangeCollection });
      rebuildDecorations();

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

        const result = await invoke<AiGenerateResult>("ai_generate", {
          prompt,
          agent: aiAgentRef.current,
          model: aiAgentRef.current === "claude-code" ? claudeCodeModelRef.current : null,
          cwd: workspaceRef.current?.path ?? null,
          cancelId: String(genId),
        });

        console.groupCollapsed(`[ai_generate] line ${startLine}`);
        console.log("temp file:", result.tempPath);
        console.log("text (from temp file):", result.text);
        if (result.stderr.trim()) console.log("stderr:", result.stderr);
        if (result.raw.trim()) console.log("raw stdout (ignored):", result.raw);
        console.groupEnd();

        // Defensive strip in case the model writes fences despite the protocol.
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
        if (String(err) !== "CANCELLED") {
          console.error("AI generation failed:", err);
        }
      } finally {
        glyphCollection.clear();
        rangeCollection.clear();
        inFlightRef.current.delete(genId);
        rebuildDecorations();
        // Function boundaries likely shifted; refetch symbols.
        scheduleAiGutterRefresh();
      }
    },
    [editorRef, monacoRef, rebuildDecorations, scheduleAiGutterRefresh],
  );

  // Keep a ref to the latest callback so the Monaco mouse handler (registered once on
  // mount) always sees the current closure without re-binding.
  const generateFunctionAtLineRef = useRef(generateFunctionAtLine);
  generateFunctionAtLineRef.current = generateFunctionAtLine;

  const handleGlyphMarginClick = useCallback(
    (e: editor.IEditorMouseEvent) => {
      if (!aiCompletionEnabledRef.current) return;
      const monaco = monacoRef.current;
      if (!monaco) return;
      if (e.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return;
      const line = e.target.position?.lineNumber;
      if (!line) return;
      if (functionsRef.current.has(line)) {
        e.event.preventDefault();
        e.event.stopPropagation();
        generateFunctionAtLineRef.current(line);
      }
    },
    [monacoRef],
  );

  useEffect(() => {
    if (!editorReady) return;
    if (aiCompletionEnabled) {
      refreshAiGutter();
    } else {
      gutterDecorationsRef.current?.clear();
      gutterDecorationsRef.current = null;
      functionsRef.current = new Map();
    }
  }, [editorReady, aiCompletionEnabled, workspace, refreshAiGutter]);

  // Cleanup debounce timer on unmount.
  useEffect(() => {
    return () => {
      if (refreshTimerRef.current != null) clearTimeout(refreshTimerRef.current);
    };
  }, []);

  return { scheduleAiGutterRefresh, handleGlyphMarginClick };
}
