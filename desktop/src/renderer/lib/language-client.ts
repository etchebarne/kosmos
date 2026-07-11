import {
  changeLanguageServerDocument,
  closeLanguageServerDocument,
  requestLanguageServerFormatting,
  saveLanguageServerDocument,
  getLanguageServerColorPresentations,
  getLanguageServerCompletions,
  getLanguageServerDiagnostics,
  getLanguageServerDocumentColors,
  getLanguageServerHover,
  listFormatters,
  listLanguageServers,
  openLanguageServerDocument,
  resolveLanguageServerCompletion,
  trustLanguageServerWorkspace,
} from "@/renderer/ipc";
import type {
  LanguageServerCompletionItem,
  LanguageServerRange,
  LanguageServerSnapshot,
  TabId,
  WorkspaceId,
} from "@/shared/ipc";
import { typescript as monacoTypeScript } from "monaco-editor/esm/vs/editor/editor.main.js";

import { monaco } from "./monaco";

export type LanguageDocumentHandle = {
  dispose(): void;
};

type LanguageDocument = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  path: string;
  generation: number;
  syncedVersion: number;
  opened: boolean;
  model: monaco.editor.ITextModel;
  queue: Promise<void>;
  completionQueue: Promise<void>;
  diagnosticsTimer: number | null;
  bindingRetryTimer: number | null;
  connectionEpoch: number;
  disposed: boolean;
};

const documents = new Map<string, LanguageDocument>();
const completionMetadata = new WeakMap<
  monaco.languages.CompletionItem,
  {
    document: LanguageDocument;
    generation: number;
    version: number;
    item: LanguageServerCompletionItem;
  }
>();
const colorMetadata = new WeakMap<
  monaco.languages.IColorInformation,
  {
    document: LanguageDocument;
    generation: number;
    version: number;
    serverId: string;
    range: LanguageServerRange;
  }
>();
const trustRequests = new Map<WorkspaceId, Promise<boolean>>();
const DIAGNOSTIC_OWNER = "kosmos-language-server";
const DIAGNOSTIC_RETRY_COUNT = 20;
const DIAGNOSTIC_RETRY_DELAY_MS = 150;
const DIAGNOSTIC_REFRESH_INTERVAL_MS = 2_000;
const BINDING_RETRY_DELAY_MS = 2_000;
const COMPLETION_TRIGGER_CHARACTERS = [
  ".",
  "(",
  "[",
  "]",
  "!",
  "/",
  "-",
  ":",
];
const originalTypeScriptDiagnostics = {
  ...monacoTypeScript.typescriptDefaults.getDiagnosticsOptions(),
};
const originalJavaScriptDiagnostics = {
  ...monacoTypeScript.javascriptDefaults.getDiagnosticsOptions(),
};
const originalTypeScriptMode = {
  ...monacoTypeScript.typescriptDefaults.modeConfiguration,
};
const originalJavaScriptMode = {
  ...monacoTypeScript.javascriptDefaults.modeConfiguration,
};
let externalTypeScriptFeaturesActive = false;
let typescriptLanguageServerInstalled = false;
let supportedLanguages = Promise.resolve(new Set<string>());
let nextGeneration = randomGenerationSeed();
let initialized = false;

export function initializeLanguageClient(): void {
  if (initialized) {
    return;
  }
  initialized = true;

  supportedLanguages = Promise.all([
    listLanguageServers().catch(() => ({ servers: [] })),
    listFormatters().catch(() => ({ formatters: [] })),
  ])
    .then(([snapshot, formatterSnapshot]) => {
      const languages = new Set(
        snapshot.servers.flatMap((server) => server.languageIds),
      );
      const documentLanguages = new Set([
        ...languages,
        ...formatterSnapshot.formatters.flatMap((formatter) => formatter.languageIds),
      ]);
      for (const language of languages) {
        monaco.languages.registerHoverProvider(
          { language, scheme: "kosmos" },
          {
            async provideHover(model, position, token) {
              const document = documents.get(model.uri.toString());
              if (!document || document.disposed || token.isCancellationRequested) {
                return null;
              }

              try {
                const hover = await afterPendingChanges(document, () =>
                  requestHover(document, position, token),
                );
                if (!hover || token.isCancellationRequested || document.disposed) {
                  return null;
                }

                return {
                  contents: hover.contents.map((content) => ({
                    value:
                      content.kind === "markdown"
                        ? content.value
                        : `\`\`\`text\n${content.value}\n\`\`\``,
                    isTrusted: false,
                    supportHtml: false,
                  })),
                  range: hover.range
                    ? new monaco.Range(
                        hover.range.start.line + 1,
                        hover.range.start.character + 1,
                        hover.range.end.line + 1,
                        hover.range.end.character + 1,
                      )
                    : undefined,
                };
              } catch {
                return null;
              }
            },
          },
        );
        monaco.languages.registerCompletionItemProvider(
          { language, scheme: "kosmos" },
          {
            triggerCharacters: COMPLETION_TRIGGER_CHARACTERS,
            async provideCompletionItems(model, position, context, token) {
              const document = documents.get(model.uri.toString());
              if (!document || document.disposed || token.isCancellationRequested) {
                return { suggestions: [] };
              }
              try {
                const result = await afterPendingChanges(document, () =>
                  requestCompletion(document, position, context, token),
                );
                if (token.isCancellationRequested || document.disposed) {
                  return { suggestions: [] };
                }
                return {
                  suggestions: result.completion.items.map((item) =>
                    monacoCompletionItem(
                      document,
                      result.generation,
                      result.version,
                      model,
                      position,
                      item,
                    ),
                  ),
                  incomplete: result.completion.isIncomplete,
                };
              } catch {
                return { suggestions: [] };
              }
            },
            async resolveCompletionItem(item, token) {
              const metadata = completionMetadata.get(item);
              if (
                !metadata ||
                metadata.document.disposed ||
                metadata.document.generation !== metadata.generation ||
                metadata.document.model.getVersionId() !== metadata.version ||
                token.isCancellationRequested
              ) {
                return item;
              }
              try {
                const resolved = await resolveLanguageServerCompletion({
                  workspaceId: metadata.document.workspaceId,
                  path: metadata.document.path,
                  generation: metadata.generation,
                  version: metadata.version,
                  serverId: metadata.item.serverId,
                  raw: metadata.item.raw,
                });
                if (
                  metadata.document.disposed ||
                  metadata.document.generation !== metadata.generation ||
                  metadata.document.model.getVersionId() !== metadata.version ||
                  token.isCancellationRequested
                ) {
                  return item;
                }
                return {
                  ...item,
                  detail: resolved.detail ?? item.detail,
                  documentation:
                    completionDocumentation(resolved) ?? item.documentation,
                  additionalTextEdits:
                    resolved.additionalTextEdits.length > 0
                      ? resolved.additionalTextEdits.map(completionAdditionalTextEdit)
                      : item.additionalTextEdits,
                };
              } catch {
                return item;
              }
            },
          },
        );
        monaco.languages.registerColorProvider(
          { language, scheme: "kosmos" },
          {
            async provideDocumentColors(model, token) {
              const document = documents.get(model.uri.toString());
              if (!document || document.disposed || token.isCancellationRequested) {
                return [];
              }
              try {
                const result = await afterPendingChanges(document, async () => {
                  await ensureOpen(document);
                  const generation = document.generation;
                  const version = document.model.getVersionId();
                  const colors = await getLanguageServerDocumentColors({
                    workspaceId: document.workspaceId,
                    path: document.path,
                    generation,
                    version,
                  });
                  return { colors, generation, version };
                });
                if (
                  token.isCancellationRequested ||
                  document.disposed ||
                  document.generation !== result.generation ||
                  document.model.getVersionId() !== result.version
                ) {
                  return [];
                }
                return result.colors.map((color) => {
                  const information: monaco.languages.IColorInformation = {
                    range: monacoRange(color.range),
                    color: color.color,
                  };
                  colorMetadata.set(information, {
                    document,
                    generation: result.generation,
                    version: result.version,
                    serverId: color.serverId,
                    range: color.range,
                  });
                  return information;
                });
              } catch {
                return [];
              }
            },
            async provideColorPresentations(model, colorInfo, token) {
              const metadata = colorMetadata.get(colorInfo);
              if (
                !metadata ||
                metadata.document.model !== model ||
                metadata.document.disposed ||
                metadata.document.generation !== metadata.generation ||
                metadata.document.model.getVersionId() !== metadata.version ||
                token.isCancellationRequested
              ) {
                return [];
              }
              try {
                const presentations = await getLanguageServerColorPresentations({
                  workspaceId: metadata.document.workspaceId,
                  path: metadata.document.path,
                  generation: metadata.generation,
                  version: metadata.version,
                  serverId: metadata.serverId,
                  range: metadata.range,
                  color: colorInfo.color,
                });
                if (
                  metadata.document.disposed ||
                  metadata.document.generation !== metadata.generation ||
                  metadata.document.model.getVersionId() !== metadata.version ||
                  token.isCancellationRequested
                ) {
                  return [];
                }
                return presentations.map((presentation) => ({
                  label: presentation.label,
                  textEdit: presentation.textEdit
                    ? {
                        range: monacoRange(presentation.textEdit.replace),
                        text: presentation.textEdit.newText,
                      }
                    : undefined,
                  additionalTextEdits: presentation.additionalTextEdits.map((edit) => ({
                    range: monacoRange(edit.replace),
                    text: edit.newText,
                  })),
                }));
              } catch {
                return [];
              }
            },
          },
        );
      }
      for (const server of snapshot.servers) {
        applyLanguageServerStatus(server);
      }
      return documentLanguages;
    })
    .catch(async () => {
      await delay(BINDING_RETRY_DELAY_MS);
      initialized = false;
      initializeLanguageClient();
      return supportedLanguages;
    });
}

export function applyLanguageServerStatus(server: LanguageServerSnapshot): void {
  const installed = server.installationState === "installed";
  if (server.id === "typescript-language-server") {
    typescriptLanguageServerInstalled = installed;
    if (!installed) {
      deactivateExternalTypeScriptFeatures();
    }
  }
  for (const document of documents.values()) {
    if (!server.languageIds.includes(document.model.getLanguageId())) {
      continue;
    }
    document.connectionEpoch += 1;
    if (document.bindingRetryTimer !== null) {
      window.clearTimeout(document.bindingRetryTimer);
      document.bindingRetryTimer = null;
    }
    if (installed) {
      document.opened = false;
      enqueue(document, () => ensureOpen(document));
    } else {
      document.opened = false;
      if (document.diagnosticsTimer !== null) {
        window.clearTimeout(document.diagnosticsTimer);
        document.diagnosticsTimer = null;
      }
      monaco.editor.setModelMarkers(document.model, DIAGNOSTIC_OWNER, []);
      enqueue(document, () => ensureOpen(document));
    }
  }
}

export async function formatLanguageDocument(
  editor: monaco.editor.IStandaloneCodeEditor,
): Promise<boolean> {
  const model = editor.getModel();
  if (!model) {
    return false;
  }
  const document = documents.get(model.uri.toString());
  if (!document || document.disposed) {
    return false;
  }
  const result = await afterPendingChanges(document, async () => {
    try {
      await ensureOpen(document);
    } catch {
      // Installed standalone formatters do not require an LSP document binding.
    }
    const generation = document.generation;
    const version = model.getVersionId();
    const options = model.getOptions();
    const edits = await requestLanguageServerFormatting({
      workspaceId: document.workspaceId,
      path: document.path,
      languageId: model.getLanguageId(),
      generation,
      version,
      text: model.getValue(),
      tabSize: options.tabSize,
      insertSpaces: options.insertSpaces,
    });
    return { edits, generation, version };
  });
  if (
    !result ||
    document.disposed ||
    document.generation !== result.generation ||
    model.getVersionId() !== result.version
  ) {
    return false;
  }
  if (result.edits.length === 0) {
    return true;
  }
  editor.pushUndoStop();
  const applied = editor.executeEdits(
    "kosmos.formatDocument",
    result.edits.map((edit) => ({
      range: monacoRange(edit.range),
      text: edit.newText,
      forceMoveMarkers: true,
    })),
  );
  editor.pushUndoStop();
  return applied;
}


export async function notifyLanguageDocumentSaved(
  model: monaco.editor.ITextModel,
  text: string,
): Promise<void> {
  const document = documents.get(model.uri.toString());
  if (!document || document.disposed) {
    return;
  }
  await afterPendingChanges(document, async () => {
    if (!document.opened || document.disposed) {
      return;
    }
    await saveLanguageServerDocument({
      workspaceId: document.workspaceId,
      path: document.path,
      generation: document.generation,
      version: document.model.getVersionId(),
      text,
    });
  });
}

export function attachLanguageDocument(
  workspaceId: WorkspaceId,
  tabId: TabId,
  path: string,
  model: monaco.editor.ITextModel,
): LanguageDocumentHandle {
  let disposed = false;
  let activeHandle: LanguageDocumentHandle | undefined;
  void supportedLanguages.then((languages) => {
    if (!disposed && languages.has(model.getLanguageId())) {
      activeHandle = attachSupportedLanguageDocument(workspaceId, tabId, path, model);
    }
  });
  return {
    dispose() {
      disposed = true;
      activeHandle?.dispose();
    },
  };
}

function attachSupportedLanguageDocument(
  workspaceId: WorkspaceId,
  tabId: TabId,
  path: string,
  model: monaco.editor.ITextModel,
): LanguageDocumentHandle {
  const document: LanguageDocument = {
    workspaceId,
    tabId,
    path,
    generation: 0,
    syncedVersion: 0,
    opened: false,
    model,
    queue: Promise.resolve(),
    completionQueue: Promise.resolve(),
    diagnosticsTimer: null,
    bindingRetryTimer: null,
    connectionEpoch: 0,
    disposed: false,
  };
  documents.set(model.uri.toString(), document);
  enqueue(document, () => ensureOpen(document));

  const changes = model.onDidChangeContent((event) => {
    const text = model.getValue();
    enqueue(document, () => synchronizeChange(document, event, text));
  });

  return {
    dispose() {
      if (document.disposed) {
        return;
      }
      document.disposed = true;
      changes.dispose();
      if (document.diagnosticsTimer !== null) {
        window.clearTimeout(document.diagnosticsTimer);
      }
      if (document.bindingRetryTimer !== null) {
        window.clearTimeout(document.bindingRetryTimer);
      }
      documents.delete(model.uri.toString());
      monaco.editor.setModelMarkers(model, DIAGNOSTIC_OWNER, []);
      enqueue(document, async () => {
        if (!document.opened) {
          return;
        }
        await closeLanguageServerDocument({
          workspaceId,
          path,
          generation: document.generation,
        });
        document.opened = false;
      });
    },
  };
}

async function ensureOpen(document: LanguageDocument): Promise<void> {
  if (document.disposed || document.opened) {
    return;
  }
  const connectionEpoch = document.connectionEpoch;
  document.generation = nextDocumentGeneration();
  const version = document.model.getVersionId();
  let complete: boolean;
  try {
    complete = await openDocument(document, version);
  } catch (caughtError) {
    if (!(await trustWorkspaceAfterError(document.workspaceId, caughtError))) {
      if (isTransientBindingError(caughtError)) {
        scheduleBindingRetry(document, connectionEpoch);
      }
      throw caughtError;
    }
    document.generation = nextDocumentGeneration();
    try {
      complete = await openDocument(document, version);
    } catch (retryError) {
      if (isTransientBindingError(retryError)) {
        scheduleBindingRetry(document, connectionEpoch);
      }
      throw retryError;
    }
  }
  const openedGeneration = document.generation;
  if (document.disposed || document.connectionEpoch !== connectionEpoch) {
    try {
      await closeLanguageServerDocument({
        workspaceId: document.workspaceId,
        path: document.path,
        generation: openedGeneration,
      });
    } catch {
      // Workspace and server cleanup also remove stale generations.
    }
    return;
  }
  document.opened = true;
  document.syncedVersion = version;
  if (!complete) {
    scheduleBindingRetry(document, connectionEpoch);
  }
  activateExternalLanguageFeatures(document.model);
  refreshDiagnostics(document, version);
}

function scheduleBindingRetry(document: LanguageDocument, connectionEpoch: number): void {
  if (
    document.disposed ||
    document.connectionEpoch !== connectionEpoch
  ) {
    return;
  }
  if (document.bindingRetryTimer !== null) {
    window.clearTimeout(document.bindingRetryTimer);
  }
  document.bindingRetryTimer = window.setTimeout(() => {
    document.bindingRetryTimer = null;
    enqueue(document, async () => {
      if (
        document.disposed ||
        document.connectionEpoch !== connectionEpoch
      ) {
        return;
      }
      try {
        if (!document.opened) {
          await ensureOpen(document);
          return;
        }
        const complete = await openDocument(document, document.model.getVersionId());
        if (!complete) {
          scheduleBindingRetry(document, connectionEpoch);
        }
      } catch {
        if (document.opened) {
          scheduleBindingRetry(document, connectionEpoch);
        }
      }
    });
  }, BINDING_RETRY_DELAY_MS);
}

function openDocument(document: LanguageDocument, version: number): Promise<boolean> {
  return openLanguageServerDocument({
    workspaceId: document.workspaceId,
    tabId: document.tabId,
    languageId: document.model.getLanguageId(),
    generation: document.generation,
    version,
    text: document.model.getValue(),
  });
}

function trustWorkspaceAfterError(workspaceId: WorkspaceId, error: unknown): Promise<boolean> {
  if (
    !(error instanceof Error) ||
    !error.message.startsWith("language_servers.workspace_not_trusted:")
  ) {
    return Promise.resolve(false);
  }
  const existing = trustRequests.get(workspaceId);
  if (existing) {
    return existing;
  }

  const request = Promise.resolve().then(async () => {
    await trustLanguageServerWorkspace({ workspaceId });
    return true;
  });
  trustRequests.set(workspaceId, request);
  void request.then(
    () => trustRequests.delete(workspaceId),
    () => trustRequests.delete(workspaceId),
  );
  return request;
}

async function synchronizeChange(
  document: LanguageDocument,
  event: monaco.editor.IModelContentChangedEvent,
  text: string,
): Promise<void> {
  if (document.disposed || event.versionId <= document.syncedVersion) {
    return;
  }
  if (!document.opened) {
    await ensureOpen(document);
    return;
  }

  try {
    monaco.editor.setModelMarkers(document.model, DIAGNOSTIC_OWNER, []);
    await changeLanguageServerDocument({
      workspaceId: document.workspaceId,
      path: document.path,
      generation: document.generation,
      version: event.versionId,
      changes: event.changes.map((change) => ({
        range: {
          start: {
            line: change.range.startLineNumber - 1,
            character: change.range.startColumn - 1,
          },
          end: {
            line: change.range.endLineNumber - 1,
            character: change.range.endColumn - 1,
          },
        },
        text: change.text,
      })),
      text,
    });
    document.syncedVersion = event.versionId;
    refreshDiagnostics(document, event.versionId);
  } catch {
    document.opened = false;
    await ensureOpen(document);
  }
}

function refreshDiagnostics(document: LanguageDocument, version: number): void {
  if (document.diagnosticsTimer !== null) {
    window.clearTimeout(document.diagnosticsTimer);
    document.diagnosticsTimer = null;
  }
  void pollDiagnostics(document, document.generation, version);
}

async function pollDiagnostics(
  document: LanguageDocument,
  generation: number,
  version: number,
): Promise<void> {
  for (let attempt = 0; attempt < DIAGNOSTIC_RETRY_COUNT; attempt += 1) {
    await delay(DIAGNOSTIC_RETRY_DELAY_MS);
    if (
      document.disposed ||
      !document.opened ||
      document.generation !== generation ||
      document.syncedVersion !== version
    ) {
      return;
    }
    try {
      const diagnostics = await getLanguageServerDiagnostics({
        workspaceId: document.workspaceId,
        path: document.path,
        generation,
        version,
      });
      if (diagnostics === null) {
        continue;
      }
      if (
        document.disposed ||
        !document.opened ||
        document.generation !== generation ||
        document.syncedVersion !== version
      ) {
        return;
      }
      monaco.editor.setModelMarkers(
        document.model,
        DIAGNOSTIC_OWNER,
        diagnostics.map((diagnostic) => ({
          startLineNumber: diagnostic.range.start.line + 1,
          startColumn: diagnostic.range.start.character + 1,
          endLineNumber: diagnostic.range.end.line + 1,
          endColumn: diagnostic.range.end.character + 1,
          severity: markerSeverity(diagnostic.severity),
          message: diagnostic.message,
          source: diagnostic.source ?? undefined,
          code: diagnostic.code ?? undefined,
        })),
      );
      scheduleDiagnosticsRefresh(document, generation, version);
      return;
    } catch {
      scheduleDiagnosticsRefresh(document, generation, version);
      return;
    }
  }
  scheduleDiagnosticsRefresh(document, generation, version);
}

function scheduleDiagnosticsRefresh(
  document: LanguageDocument,
  generation: number,
  version: number,
): void {
  if (
    document.disposed ||
    !document.opened ||
    document.generation !== generation ||
    document.syncedVersion !== version
  ) {
    return;
  }
  if (document.diagnosticsTimer !== null) {
    window.clearTimeout(document.diagnosticsTimer);
  }
  document.diagnosticsTimer = window.setTimeout(() => {
    document.diagnosticsTimer = null;
    void pollDiagnostics(document, generation, version);
  }, DIAGNOSTIC_REFRESH_INTERVAL_MS);
}

function markerSeverity(
  severity: "error" | "warning" | "information" | "hint" | null,
): monaco.MarkerSeverity {
  switch (severity) {
    case "error":
      return monaco.MarkerSeverity.Error;
    case "warning":
      return monaco.MarkerSeverity.Warning;
    case "hint":
      return monaco.MarkerSeverity.Hint;
    case "information":
    case null:
      return monaco.MarkerSeverity.Info;
  }
}

function activateExternalLanguageFeatures(model: monaco.editor.ITextModel): void {
  const language = model.getLanguageId();
  if (
    !typescriptLanguageServerInstalled ||
    (language !== "typescript" && language !== "javascript")
  ) {
    return;
  }
  if (externalTypeScriptFeaturesActive) {
    monaco.editor.setModelMarkers(model, language, []);
    return;
  }
  externalTypeScriptFeaturesActive = true;
  for (const defaults of [
    monacoTypeScript.typescriptDefaults,
    monacoTypeScript.javascriptDefaults,
  ]) {
    defaults.setDiagnosticsOptions({
      ...defaults.getDiagnosticsOptions(),
      noSemanticValidation: true,
      noSyntaxValidation: true,
      noSuggestionDiagnostics: true,
    });
    defaults.setModeConfiguration({
      ...defaults.modeConfiguration,
      completionItems: false,
      diagnostics: false,
      hovers: false,
    });
  }
  for (const document of documents.values()) {
    const documentLanguage = document.model.getLanguageId();
    if (documentLanguage === "typescript" || documentLanguage === "javascript") {
      monaco.editor.setModelMarkers(document.model, documentLanguage, []);
    }
  }
}

function deactivateExternalTypeScriptFeatures(): void {
  if (!externalTypeScriptFeaturesActive) {
    return;
  }
  externalTypeScriptFeaturesActive = false;
  monacoTypeScript.typescriptDefaults.setDiagnosticsOptions(originalTypeScriptDiagnostics);
  monacoTypeScript.javascriptDefaults.setDiagnosticsOptions(originalJavaScriptDiagnostics);
  monacoTypeScript.typescriptDefaults.setModeConfiguration(originalTypeScriptMode);
  monacoTypeScript.javascriptDefaults.setModeConfiguration(originalJavaScriptMode);
}

async function requestCompletion(
  document: LanguageDocument,
  position: monaco.Position,
  context: monaco.languages.CompletionContext,
  token: monaco.CancellationToken,
) {
  return serializeCompletion(document, async () => {
    await ensureOpen(document);
    if (!document.opened || token.isCancellationRequested) {
      return emptyCompletion(document);
    }
    const generation = document.generation;
    const version = document.model.getVersionId();
    const completion = await getLanguageServerCompletions({
      workspaceId: document.workspaceId,
      path: document.path,
      generation,
      version,
      position: {
        line: position.lineNumber - 1,
        character: position.column - 1,
      },
      triggerKind: context.triggerKind + 1,
      triggerCharacter: context.triggerCharacter ?? null,
      filter: completionFilter(document.model, position),
    });
    if (
      token.isCancellationRequested ||
      document.disposed ||
      document.generation !== generation ||
      document.model.getVersionId() !== version
    ) {
      return emptyCompletion(document, generation, version);
    }
    return { completion, generation, version };
  });
}

function emptyCompletion(
  document: LanguageDocument,
  generation = document.generation,
  version = document.model.getVersionId(),
) {
  return {
    completion: { items: [], isIncomplete: false },
    generation,
    version,
  };
}

function completionFilter(
  model: monaco.editor.ITextModel,
  position: monaco.Position,
): string {
  const line = model.getLineContent(position.lineNumber).slice(0, position.column - 1);
  return line.match(/[^\s"'`(){}\[\]]+$/)?.[0] ?? "";
}

async function serializeCompletion<T>(
  document: LanguageDocument,
  operation: () => Promise<T>,
): Promise<T> {
  const previous = document.completionQueue;
  let release = () => {};
  const current = new Promise<void>((resolve) => {
    release = resolve;
  });
  document.completionQueue = previous.then(() => current);
  await previous;
  try {
    return await operation();
  } finally {
    release();
  }
}

function monacoCompletionItem(
  document: LanguageDocument,
  generation: number,
  version: number,
  model: monaco.editor.ITextModel,
  position: monaco.Position,
  item: LanguageServerCompletionItem,
): monaco.languages.CompletionItem {
  const edit = item.textEdit;
  const word = model.getWordUntilPosition(position);
  const completion: monaco.languages.CompletionItem = {
    label:
      item.labelDetail || item.labelDescription
        ? {
            label: item.label,
            detail: item.labelDetail ?? undefined,
            description: item.labelDescription ?? undefined,
          }
        : item.label,
    kind: completionKind(item.kind),
    tags: item.deprecated ? [monaco.languages.CompletionItemTag.Deprecated] : undefined,
    detail: item.detail ?? undefined,
    documentation: completionDocumentation(item),
    sortText: item.sortText ?? undefined,
    filterText: item.filterText ?? undefined,
    preselect: item.preselect,
    insertText: edit?.newText ?? item.insertText,
    insertTextRules: item.insertTextIsSnippet
      ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
      : undefined,
    range: edit
      ? {
          insert: monacoRange(edit.insert),
          replace: monacoRange(edit.replace),
        }
      : new monaco.Range(
          position.lineNumber,
          word.startColumn,
          position.lineNumber,
          word.endColumn,
        ),
    commitCharacters:
      item.commitCharacters.length > 0 ? item.commitCharacters : undefined,
    additionalTextEdits: item.additionalTextEdits.map(completionAdditionalTextEdit),
  };
  completionMetadata.set(completion, {
    document,
    generation,
    version,
    item,
  });
  return completion;
}

function completionDocumentation(item: LanguageServerCompletionItem) {
  if (!item.documentation) {
    return undefined;
  }
  return {
    value:
      item.documentation.kind === "markdown"
        ? item.documentation.value
        : `\`\`\`text\n${item.documentation.value}\n\`\`\``,
    isTrusted: false,
    supportHtml: false,
  };
}

function completionAdditionalTextEdit(
  edit: LanguageServerCompletionItem["additionalTextEdits"][number],
): monaco.editor.ISingleEditOperation {
  return {
    range: monacoRange(edit.replace),
    text: edit.newText,
  };
}

function monacoRange(range: {
  start: { line: number; character: number };
  end: { line: number; character: number };
}): monaco.Range {
  return new monaco.Range(
    range.start.line + 1,
    range.start.character + 1,
    range.end.line + 1,
    range.end.character + 1,
  );
}

function completionKind(kind: number | null): monaco.languages.CompletionItemKind {
  const kinds = monaco.languages.CompletionItemKind;
  switch (kind) {
    case 2:
      return kinds.Method;
    case 3:
      return kinds.Function;
    case 4:
      return kinds.Constructor;
    case 5:
      return kinds.Field;
    case 6:
      return kinds.Variable;
    case 7:
      return kinds.Class;
    case 8:
      return kinds.Interface;
    case 9:
      return kinds.Module;
    case 10:
      return kinds.Property;
    case 11:
      return kinds.Unit;
    case 12:
      return kinds.Value;
    case 13:
      return kinds.Enum;
    case 14:
      return kinds.Keyword;
    case 15:
      return kinds.Snippet;
    case 16:
      return kinds.Color;
    case 17:
      return kinds.File;
    case 18:
      return kinds.Reference;
    case 19:
      return kinds.Folder;
    case 20:
      return kinds.EnumMember;
    case 21:
      return kinds.Constant;
    case 22:
      return kinds.Struct;
    case 23:
      return kinds.Event;
    case 24:
      return kinds.Operator;
    case 25:
      return kinds.TypeParameter;
    default:
      return kinds.Text;
  }
}

async function requestHover(
  document: LanguageDocument,
  position: monaco.Position,
  token: monaco.CancellationToken,
) {
  await ensureOpen(document);
  if (!document.opened || token.isCancellationRequested) {
    return null;
  }

  const generation = document.generation;
  const version = document.model.getVersionId();
  const hover = await getLanguageServerHover({
    workspaceId: document.workspaceId,
    path: document.path,
    generation,
    version,
    position: {
      line: position.lineNumber - 1,
      character: position.column - 1,
    },
  });
  return !token.isCancellationRequested &&
    document.generation === generation &&
    document.model.getVersionId() === version
    ? hover
    : null;
}

function nextDocumentGeneration(): number {
  nextGeneration = (nextGeneration + 1) % Number.MAX_SAFE_INTEGER;
  return nextGeneration;
}

function randomGenerationSeed(): number {
  const values = crypto.getRandomValues(new Uint32Array(2));
  return ((values[0] ?? 0) & 0x1f_ffff) * 0x1_0000_0000 + (values[1] ?? 0);
}

function isTransientBindingError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false;
  }
  return [
    "language_servers.worker_busy:",
    "language_servers.worker_unavailable:",
    "language_servers.server_start_failed:",
    "language_servers.server_exited:",
    "language_servers.request_timeout:",
    "language_servers.runtime_unavailable:",
  ].some((prefix) => error.message.startsWith(prefix));
}

function enqueue(document: LanguageDocument, operation: () => Promise<unknown>): void {
  document.queue = document.queue.then(operation).then(
    () => undefined,
    () => undefined,
  );
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

async function afterPendingChanges<T>(
  document: LanguageDocument,
  operation: () => Promise<T>,
): Promise<T> {
  await document.queue;
  return operation();
}
