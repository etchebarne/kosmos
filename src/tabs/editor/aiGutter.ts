import type { editor } from "monaco-editor";
import type {
  DocumentSymbol,
  Range as LspRange,
  SymbolInformation,
} from "vscode-languageserver-protocol";

// LSP SymbolKind values that represent function-like definitions.
const FUNCTION_KINDS = new Set<number>([6, 9, 12]);

export const AI_GENERATE_GLYPH_CLASS = "ai-generate-glyph";
export const AI_GENERATE_GLYPH_LOADING_CLASS = "ai-generate-glyph-loading";

export interface AiFunctionInfo {
  /** Full range of the function definition, 1-based Monaco coords. */
  range: {
    startLineNumber: number;
    startColumn: number;
    endLineNumber: number;
    endColumn: number;
  };
}

function lspRangeToMonaco(range: LspRange): AiFunctionInfo["range"] {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}

function collectFunctions(
  symbols: DocumentSymbol[] | SymbolInformation[] | null,
  out: Map<number, AiFunctionInfo>,
): void {
  if (!symbols) return;
  for (const sym of symbols) {
    const kind = (sym as DocumentSymbol).kind;
    const fullRange = (sym as DocumentSymbol).range ?? (sym as SymbolInformation).location?.range;
    const selectionRange = (sym as DocumentSymbol).selectionRange;
    const startLineSource = selectionRange ?? fullRange;
    if (FUNCTION_KINDS.has(kind) && fullRange && startLineSource) {
      const startLine = startLineSource.start.line + 1;
      if (!out.has(startLine)) {
        out.set(startLine, { range: lspRangeToMonaco(fullRange) });
      }
    }
    const children = (sym as DocumentSymbol).children;
    if (children && children.length > 0) {
      collectFunctions(children, out);
    }
  }
}

export function extractFunctions(
  symbols: DocumentSymbol[] | SymbolInformation[] | null,
): Map<number, AiFunctionInfo> {
  const out = new Map<number, AiFunctionInfo>();
  collectFunctions(symbols, out);
  return out;
}

export interface WindowedContext {
  text: string;
  /** Line range in the original file that the context covers (1-based, inclusive). */
  windowStart: number;
  windowEnd: number;
  headerEnd: number;
  totalLines: number;
}

/**
 * Build a windowed view of the file: the top `headerLines` (for imports / top-level
 * declarations) plus `before`/`after` lines around the target range. Overlapping
 * windows are merged. Keeps prompts small so concurrent generations stay cheap.
 */
export function buildWindowedContext(
  model: editor.ITextModel,
  target: { startLineNumber: number; endLineNumber: number },
  opts: { headerLines?: number; before?: number; after?: number } = {},
): WindowedContext {
  const headerLines = opts.headerLines ?? 40;
  const before = opts.before ?? 80;
  const after = opts.after ?? 80;
  const total = model.getLineCount();

  const headerEnd = Math.min(headerLines, total);
  const windowStart = Math.max(1, target.startLineNumber - before);
  const windowEnd = Math.min(total, target.endLineNumber + after);

  const readRange = (start: number, end: number) =>
    model.getValueInRange({
      startLineNumber: start,
      startColumn: 1,
      endLineNumber: end,
      endColumn: model.getLineMaxColumn(end),
    });

  if (windowStart <= headerEnd + 1) {
    // Windows touch or overlap — emit as a single slice.
    return {
      text: readRange(1, windowEnd),
      windowStart: 1,
      windowEnd,
      headerEnd,
      totalLines: total,
    };
  }

  const header = readRange(1, headerEnd);
  const windowText = readRange(windowStart, windowEnd);
  const omitted = windowStart - headerEnd - 1;
  return {
    text: `${header}\n\n// ... (omitted ${omitted} lines) ...\n\n${windowText}`,
    windowStart,
    windowEnd,
    headerEnd,
    totalLines: total,
  };
}

export function buildGenerationPrompt(opts: {
  filePath: string;
  language: string;
  context: WindowedContext;
  functionText: string;
  functionStartLine: number;
  functionEndLine: number;
}): string {
  const { filePath, language, context, functionText, functionStartLine, functionEndLine } = opts;
  return [
    "Task: the SELECTION_CONTENT below is a stub function — empty body, placeholder, or TODO.",
    "Your job: write a real, working implementation of this function and save it to TEMP_FILE (see <MustObey>).",
    "",
    "Requirements:",
    "- Preserve the existing signature (name, parameters, return type) EXACTLY.",
    "- Match the style, indentation, and naming conventions of FILE_CONTAINING_SELECTION.",
    "- Write the COMPLETE function (signature + body) to TEMP_FILE — not just the body.",
    "- Use Read / Grep / Glob to explore the workspace for types, helpers, and conventions before guessing.",
    "",
    `<SELECTION_LOCATION>${filePath} lines ${functionStartLine}–${functionEndLine}</SELECTION_LOCATION>`,
    `<SELECTION_LANGUAGE>${language}</SELECTION_LANGUAGE>`,
    "<SELECTION_CONTENT>",
    functionText,
    "</SELECTION_CONTENT>",
    "",
    `<FILE_CONTAINING_SELECTION note="windowed: lines ${context.windowStart}-${context.windowEnd} of ${context.totalLines}">`,
    context.text,
    "</FILE_CONTAINING_SELECTION>",
  ].join("\n");
}

/** Strip surrounding ```lang ... ``` fences if the model added them despite being told not to. */
export function stripCodeFences(text: string): string {
  const trimmed = text.trim();
  const fenceMatch = trimmed.match(/^```[a-zA-Z0-9_+-]*\n([\s\S]*?)\n```$/);
  return fenceMatch ? fenceMatch[1] : trimmed;
}

/**
 * Normal (non-loading) glyphs. `excludeLines` lets callers skip lines already covered
 * by a dedicated sticky loading-glyph decoration, preventing double-render.
 */
export function buildAiGutterDecorations(
  functions: Map<number, AiFunctionInfo>,
  excludeLines: Set<number> = new Set(),
): editor.IModelDeltaDecoration[] {
  const out: editor.IModelDeltaDecoration[] = [];
  for (const line of functions.keys()) {
    if (excludeLines.has(line)) continue;
    out.push({
      range: { startLineNumber: line, startColumn: 1, endLineNumber: line, endColumn: 1 },
      options: {
        glyphMarginClassName: AI_GENERATE_GLYPH_CLASS,
        glyphMarginHoverMessage: { value: "Generate function body with AI" },
      },
    });
  }
  return out;
}
