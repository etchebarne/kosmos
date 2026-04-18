import type { Monaco } from "@monaco-editor/react";
import type { IDisposable, IRange, IMarkdownString, languages } from "monaco-editor";
import {
  type TextEdit,
  CompletionItemKind as LspCompletionItemKind,
  DiagnosticSeverity,
} from "vscode-languageserver-protocol";
import type { LspClient } from "./client";

// LSP uses 0-based line/char; Monaco uses 1-based line/column.

export function toMonacoRange(range: {
  start: { line: number; character: number };
  end: { line: number; character: number };
}): IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}

export function toLspPosition(position: { lineNumber: number; column: number }) {
  return {
    line: position.lineNumber - 1,
    character: position.column - 1,
  };
}

export function toLspRange(range: IRange) {
  return {
    start: { line: range.startLineNumber - 1, character: range.startColumn - 1 },
    end: { line: range.endLineNumber - 1, character: range.endColumn - 1 },
  };
}

export const completionKindMap: Record<number, number> = {
  [LspCompletionItemKind.Text]: 18,
  [LspCompletionItemKind.Method]: 0,
  [LspCompletionItemKind.Function]: 1,
  [LspCompletionItemKind.Constructor]: 2,
  [LspCompletionItemKind.Field]: 3,
  [LspCompletionItemKind.Variable]: 4,
  [LspCompletionItemKind.Class]: 5,
  [LspCompletionItemKind.Interface]: 7,
  [LspCompletionItemKind.Module]: 8,
  [LspCompletionItemKind.Property]: 9,
  [LspCompletionItemKind.Unit]: 12,
  [LspCompletionItemKind.Value]: 13,
  [LspCompletionItemKind.Enum]: 15,
  [LspCompletionItemKind.Keyword]: 17,
  [LspCompletionItemKind.Snippet]: 27,
  [LspCompletionItemKind.Color]: 19,
  [LspCompletionItemKind.File]: 20,
  [LspCompletionItemKind.Reference]: 21,
  [LspCompletionItemKind.Folder]: 23,
  [LspCompletionItemKind.EnumMember]: 16,
  [LspCompletionItemKind.Constant]: 14,
  [LspCompletionItemKind.Struct]: 6,
  [LspCompletionItemKind.Event]: 10,
  [LspCompletionItemKind.Operator]: 11,
  [LspCompletionItemKind.TypeParameter]: 24,
};

export function toMonacoSeverity(monaco: Monaco, severity?: DiagnosticSeverity): number {
  switch (severity) {
    case DiagnosticSeverity.Error:
      return monaco.MarkerSeverity.Error;
    case DiagnosticSeverity.Warning:
      return monaco.MarkerSeverity.Warning;
    case DiagnosticSeverity.Information:
      return monaco.MarkerSeverity.Info;
    case DiagnosticSeverity.Hint:
      return monaco.MarkerSeverity.Hint;
    default:
      return monaco.MarkerSeverity.Info;
  }
}

export function toMarkdownString(
  content: string | { kind: string; value: string } | { language: string; value: string },
): IMarkdownString {
  if (typeof content === "string") {
    return { value: content };
  }
  if ("kind" in content) {
    return { value: content.value };
  }
  return { value: `\`\`\`${content.language}\n${content.value}\n\`\`\`` };
}

export function markerFingerprint(m: {
  severity: number;
  message: string;
  startLineNumber: number;
  startColumn: number;
  endLineNumber: number;
  endColumn: number;
}): string {
  return `${m.severity}:${m.startLineNumber}:${m.startColumn}:${m.endLineNumber}:${m.endColumn}:${m.message}`;
}

export function lspTextEditsToMonaco(edits: TextEdit[]): { range: IRange; text: string }[] {
  return edits.map((edit) => ({
    range: toMonacoRange(edit.range),
    text: edit.newText,
  }));
}

export function workspaceEditToMonaco(
  monaco: Monaco,
  client: LspClient,
  wsEdit: { changes?: Record<string, TextEdit[]>; documentChanges?: unknown[] },
): languages.IWorkspaceTextEdit[] {
  const edits: languages.IWorkspaceTextEdit[] = [];

  if (wsEdit.changes) {
    for (const [editUri, textEdits] of Object.entries(wsEdit.changes)) {
      for (const te of textEdits) {
        edits.push({
          resource: monaco.Uri.parse(client.fromServerUri(editUri)),
          textEdit: { range: toMonacoRange(te.range), text: te.newText },
          versionId: undefined,
        });
      }
    }
  }

  if (wsEdit.documentChanges) {
    for (const change of wsEdit.documentChanges) {
      const c = change as { textDocument?: { uri: string }; edits?: TextEdit[] };
      if (c.textDocument && c.edits) {
        for (const te of c.edits) {
          edits.push({
            resource: monaco.Uri.parse(client.fromServerUri(c.textDocument.uri)),
            textEdit: { range: toMonacoRange(te.range), text: te.newText },
            versionId: undefined,
          });
        }
      }
    }
  }

  return edits;
}

export async function safeLspCall<T>(
  label: string,
  languageId: string,
  fallback: T,
  fn: () => Promise<T>,
): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    console.warn(`[LSP] ${label} failed for ${languageId}:`, e);
    return fallback;
  }
}

export function registerIfCapable(
  capability: unknown,
  disposables: IDisposable[],
  register: () => IDisposable,
): void {
  if (!capability) return;
  disposables.push(register());
}
