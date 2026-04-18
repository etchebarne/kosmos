import type { Monaco } from "@monaco-editor/react";

let extMap: Record<string, string> = {};

/**
 * Build the extension → languageId map from Monaco's registered languages.
 * Call once after setupMonacoLanguages() so additional registrations
 * (typescriptreact, etc.) are included. When multiple languages claim
 * the same extension, the most specific (fewest total extensions) wins —
 * matching resolveModelLanguage's logic.
 */
export function initExtMap(monaco: Monaco): void {
  const langs = monaco.languages.getLanguages();
  const bestCount: Record<string, number> = {};
  extMap = {};

  for (const lang of langs) {
    if (!lang.extensions) continue;
    const count = lang.extensions.length;
    for (const ext of lang.extensions) {
      const bare = ext.startsWith(".") ? ext.slice(1).toLowerCase() : ext.toLowerCase();
      if (!(bare in extMap) || count < bestCount[bare]) {
        extMap[bare] = lang.id;
        bestCount[bare] = count;
      }
    }
  }
}

/** Resolve a file extension (without dot) to a language ID. Returns null if unknown. */
export function languageIdFromExt(ext: string): string | null {
  return extMap[ext.toLowerCase()] ?? null;
}
