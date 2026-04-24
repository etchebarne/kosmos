import type { Monaco } from "@monaco-editor/react";
import { defineKosmosTheme } from "./monacoTheme";
import { setupMonacoLanguages } from "../../lib/lsp/monacoLanguages";
import { initExtMap } from "../../lib/extToLang";
import { registerEditorOpener } from "./editorOpener";

let initialized = false;

/**
 * Run all one-time Monaco setup: theme, language registrations, extension map, and the
 * editor-opener that lets peek-at-definitions land in a kosmos tab. Idempotent — the
 * underlying setup helpers each guard against double-registration but it's cheaper to
 * short-circuit here.
 */
export function initMonaco(monaco: Monaco): void {
  if (initialized) return;
  initialized = true;
  defineKosmosTheme(monaco);
  setupMonacoLanguages(monaco);
  initExtMap(monaco);
  registerEditorOpener(monaco);

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
