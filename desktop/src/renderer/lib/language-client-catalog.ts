import type { FormatterSnapshot, LanguageServerSnapshot } from "@/shared/ipc";

export type MonacoLanguageFeature =
  | "completionItems"
  | "hovers"
  | "signatureHelp"
  | "definitions"
  | "references"
  | "documentSymbols"
  | "diagnostics"
  | "colors"
  | "documentFormattingEdits"
  | "rename"
  | "codeActions";

const PRIMARY_SERVER_FEATURES: Record<
  string,
  Record<string, readonly MonacoLanguageFeature[]>
> = {
  "typescript-language-server": {
    typescript: ["completionItems", "hovers", "signatureHelp", "definitions", "references", "documentSymbols", "diagnostics", "rename", "codeActions"],
    javascript: ["completionItems", "hovers", "signatureHelp", "definitions", "references", "documentSymbols", "diagnostics", "rename", "codeActions"],
  },
  "json-language-server": {
    json: ["completionItems", "hovers", "documentSymbols", "diagnostics", "colors", "documentFormattingEdits"],
  },
  "css-language-server": {
    css: ["completionItems", "hovers", "definitions", "references", "documentSymbols", "diagnostics", "colors", "documentFormattingEdits", "rename"],
    scss: ["completionItems", "hovers", "definitions", "references", "documentSymbols", "diagnostics", "colors", "documentFormattingEdits", "rename"],
    less: ["completionItems", "hovers", "definitions", "references", "documentSymbols", "diagnostics", "colors", "documentFormattingEdits", "rename"],
  },
  "html-language-server": {
    html: ["completionItems", "hovers", "documentSymbols", "diagnostics", "colors", "documentFormattingEdits", "rename"],
  },
};

export function activeSelectedInstallation(server: LanguageServerSnapshot): string | null {
  return server.selectedVersion !== null && server.selectedVersion === server.installedVersion
    ? server.selectedVersion
    : null;
}

export function languageServerRuntimeAvailable(server: LanguageServerSnapshot): boolean {
  return server.runtimeState === "running"
    || server.runtimeState === "restarting"
    || (server.runtimeState === "degraded" && server.sessionCount > 0);
}

export function activePrimaryFeatures(
  servers: Iterable<LanguageServerSnapshot>,
): Map<string, ReadonlySet<MonacoLanguageFeature>> {
  const features = new Map<string, Set<MonacoLanguageFeature>>();
  for (const server of servers) {
    if (activeSelectedInstallation(server) === null || !languageServerRuntimeAvailable(server)) {
      continue;
    }
    const serverFeatures = PRIMARY_SERVER_FEATURES[server.id];
    if (!serverFeatures) {
      continue;
    }
    for (const [language, languageFeatures] of Object.entries(serverFeatures)) {
      const active = features.get(language) ?? new Set<MonacoLanguageFeature>();
      languageFeatures.forEach((feature) => active.add(feature));
      features.set(language, active);
    }
  }
  return features;
}

export function activeExternalLanguages(
  servers: Iterable<LanguageServerSnapshot>,
): Set<string> {
  const languages = new Set<string>();
  for (const server of servers) {
    if (activeSelectedInstallation(server) && languageServerRuntimeAvailable(server)) {
      server.languageIds.forEach((language) => languages.add(language));
    }
  }
  return languages;
}

export function formatterApplies(
  formatter: Pick<
    FormatterSnapshot,
    "installedVersion" | "installationState" | "languageIds" | "extensions" | "filenames"
  >,
  languageId: string,
  path: string,
): boolean {
  if (formatter.installedVersion === null || formatter.installationState === "uninstalling") {
    return false;
  }
  const filename = path.slice(path.lastIndexOf("/") + 1);
  return formatter.languageIds.includes(languageId)
    || formatter.filenames.includes(filename)
    || formatter.extensions.some((extension) => filename.endsWith(extension));
}

export function documentAttachmentAction(
  attached: boolean,
  supported: boolean,
): "attach" | "detach" | "keep" {
  if (attached === supported) {
    return "keep";
  }
  return supported ? "attach" : "detach";
}

export function documentIsSupported(
  serverLanguages: ReadonlySet<string>,
  formatters: readonly FormatterSnapshot[],
  languageId: string,
  path: string,
): boolean {
  return serverLanguages.has(languageId)
    || formatters.some((formatter) => formatterApplies(formatter, languageId, path));
}

export function discoverProviderLanguages(
  registered: Set<string>,
  servers: Iterable<Pick<LanguageServerSnapshot, "languageIds">>,
): string[] {
  const discovered: string[] = [];
  for (const server of servers) {
    for (const language of server.languageIds) {
      if (registered.has(language)) {
        continue;
      }
      registered.add(language);
      discovered.push(language);
    }
  }
  return discovered;
}
