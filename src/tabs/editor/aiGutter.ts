import type { editor } from "monaco-editor";
import type { DocumentSymbol, SymbolInformation } from "vscode-languageserver-protocol";

// LSP SymbolKind values that represent function-like definitions.
const FUNCTION_KINDS = new Set<number>([6, 9, 12]);

export const AI_GENERATE_GLYPH_CLASS = "ai-generate-glyph";

function collectFunctionLines(
  symbols: DocumentSymbol[] | SymbolInformation[] | null,
  out: Set<number>,
): void {
  if (!symbols) return;
  for (const sym of symbols) {
    const kind = (sym as DocumentSymbol).kind;
    const range =
      (sym as DocumentSymbol).selectionRange ??
      (sym as DocumentSymbol).range ??
      (sym as SymbolInformation).location?.range;
    if (range && FUNCTION_KINDS.has(kind)) {
      out.add(range.start.line + 1);
    }
    const children = (sym as DocumentSymbol).children;
    if (children && children.length > 0) {
      collectFunctionLines(children, out);
    }
  }
}

export function buildAiGutterDecorations(
  symbols: DocumentSymbol[] | SymbolInformation[] | null,
): editor.IModelDeltaDecoration[] {
  const lines = new Set<number>();
  collectFunctionLines(symbols, lines);
  return Array.from(lines).map((line) => ({
    range: { startLineNumber: line, startColumn: 1, endLineNumber: line, endColumn: 1 },
    options: {
      glyphMarginClassName: AI_GENERATE_GLYPH_CLASS,
      glyphMarginHoverMessage: { value: "Generate function body with AI" },
    },
  }));
}

export function extractFunctionLines(
  symbols: DocumentSymbol[] | SymbolInformation[] | null,
): Set<number> {
  const lines = new Set<number>();
  collectFunctionLines(symbols, lines);
  return lines;
}
