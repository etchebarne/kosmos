import type { ResolvedToolingDocumentPayload } from "@/shared/ipc";

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

const MONACO_FEATURES: Record<
  ResolvedToolingDocumentPayload["features"][number]["feature"],
  MonacoLanguageFeature
> = {
  completion: "completionItems",
  hover: "hovers",
  signatureHelp: "signatureHelp",
  navigation: "definitions",
  references: "references",
  symbols: "documentSymbols",
  diagnostics: "diagnostics",
  colors: "colors",
  formatting: "documentFormattingEdits",
  rename: "rename",
  codeActions: "codeActions",
};

export function monacoFeatures(
  document: Pick<ResolvedToolingDocumentPayload, "features">,
): ReadonlySet<MonacoLanguageFeature> {
  return new Set(document.features.map(({ feature }) => MONACO_FEATURES[feature]));
}

export function acceptsToolingRevision(current: number | undefined, incoming: number): boolean {
  return current === undefined || incoming > current;
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

export function discoverProviderLanguages(
  registered: Set<string>,
  documents: Iterable<Pick<ResolvedToolingDocumentPayload, "languageId" | "supported">>,
): string[] {
  const discovered: string[] = [];
  for (const document of documents) {
    if (!document.supported || registered.has(document.languageId)) {
      continue;
    }
    registered.add(document.languageId);
    discovered.push(document.languageId);
  }
  return discovered;
}
