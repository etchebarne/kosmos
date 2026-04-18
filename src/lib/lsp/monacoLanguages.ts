import type { Monaco } from "@monaco-editor/react";
import type { editor, languages } from "monaco-editor";
import {
  conf as tsConf,
  language as tsLanguage,
} from "monaco-editor/esm/vs/basic-languages/typescript/typescript";

/** TS tokenizer + JSX/TSX tag rules; intrinsic tags get "tag", generics stay "keyword". */
function createJsxTokenizer(tokenPostfix: string): languages.IMonarchLanguage {
  const lang = structuredClone(tsLanguage);
  lang.tokenPostfix = tokenPostfix;

  const root = lang.tokenizer.root as unknown[];
  const tagAction = { cases: { "@keywords": "keyword", "@default": "tag" } };

  root.unshift(
    [/(<\/)([a-z][\w$-]*)/, ["delimiter", tagAction]],
    [/(<)([a-z][\w$-]*)/, ["delimiter", tagAction]],
  );

  return lang;
}

// Extra ids so didOpen sends the LSP the right languageId (e.g. typescriptreact vs typescript).
const ADDITIONAL_LANGUAGES = [
  {
    id: "typescriptreact",
    extensions: [".tsx"],
    conf: tsConf,
    language: createJsxTokenizer(".tsx"),
  },
  {
    id: "javascriptreact",
    extensions: [".jsx"],
    conf: tsConf,
    language: createJsxTokenizer(".jsx"),
  },
];

let registered = false;

/** Call before models are created (beforeMount). Idempotent. */
export function setupMonacoLanguages(monaco: Monaco): void {
  if (registered) return;
  registered = true;

  for (const lang of ADDITIONAL_LANGUAGES) {
    if (
      monaco.languages
        .getLanguages()
        .some((l: languages.ILanguageExtensionPoint) => l.id === lang.id)
    )
      continue;

    monaco.languages.register({ id: lang.id, extensions: lang.extensions });
    monaco.languages.setMonarchTokensProvider(lang.id, lang.language);
    monaco.languages.setLanguageConfiguration(lang.id, lang.conf);
  }
}

/** Pick the narrowest registered language for the model's extension (e.g. .tsx → typescriptreact). */
export function resolveModelLanguage(monaco: Monaco, model: editor.ITextModel): void {
  const uri = model.uri.toString();
  const extMatch = uri.match(/\.([^./?#]+)(?:[?#]|$)/);
  if (!extMatch) return;

  const ext = "." + extMatch[1].toLowerCase();
  const candidates = monaco.languages
    .getLanguages()
    .filter((l: languages.ILanguageExtensionPoint) => l.extensions?.includes(ext));

  if (candidates.length <= 1) return;

  // Fewer registered extensions = tighter fit.
  candidates.sort(
    (a: languages.ILanguageExtensionPoint, b: languages.ILanguageExtensionPoint) =>
      (a.extensions?.length ?? 0) - (b.extensions?.length ?? 0),
  );

  const best = candidates[0].id;
  if (best !== model.getLanguageId()) {
    monaco.editor.setModelLanguage(model, best);
  }
}
