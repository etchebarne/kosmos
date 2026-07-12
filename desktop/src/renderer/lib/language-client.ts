import {
  acknowledgeWorkspaceEditCompletion,
  changeLanguageServerDocument,
  closeLanguageServerDocument,
  requestLanguageServerFormatting,
  saveLanguageServerDocument,
  getLanguageServerColorPresentations,
  getLanguageServerCompletions,
  getLanguageServerDiagnostics,
  getLanguageServerDocumentColors,
  getLanguageServerHover,
  getLanguageServerSignatureHelp,
  getLanguageServerDefinitions,
  getLanguageServerDeclarations,
  getLanguageServerTypeDefinitions,
  getLanguageServerImplementations,
  getLanguageServerReferences,
  getLanguageServerDocumentSymbols,
  getLanguageServerStatus,
  getWorkspaceEditStatus,
  isRequestCancelledError,
  listFormatters,
  listLanguageServers,
  listWorkspaceEditRecoveries,
  openLanguageServerDocument,
  resolveLanguageServerCompletion,
  commitWorkspaceEdit,
  executeLanguageServerCommand,
  finalizeWorkspaceEdit,
  finishWorkspaceEdit,
  getLanguageServerCodeActions,
  prepareLanguageServerRename,
  requestLanguageServerRename,
  resolveLanguageServerCodeAction,
  rollbackWorkspaceEdit,
  stageLanguageServerCodeAction,
  trustLanguageServerWorkspace,
} from "@/renderer/ipc";
import type {
  FormatterSnapshot,
  LanguageServerCompletionItem,
  LanguageServerDiagnosticsChanged,
  LanguageServerRange,
  LanguageServerSnapshot,
  TabId,
  WorkspaceId,
  LanguageServerDocumentSymbol,
  LanguageServerLocation,
  LanguageServerCodeAction,
  KosmosServerNotification,
  StagedWorkspaceEdit,
  WorkspaceEditRecovery,
} from "@/shared/ipc";
import {
  css as monacoCss,
  html as monacoHtml,
  json as monacoJson,
  typescript as monacoTypeScript,
} from "monaco-editor/esm/vs/editor/editor.main.js";

import {
  activePrimaryFeatures,
  activeSelectedInstallation,
  activeExternalLanguages,
  documentAttachmentAction,
  documentIsSupported,
  discoverProviderLanguages,
  type MonacoLanguageFeature,
} from "./language-client-catalog";
import { isCurrentDiagnostics } from "./language-diagnostics";
import { isCurrentLanguageResult } from "./language-feature-adapters";
import { monaco } from "./monaco";
import { createQueuedRefresh } from "./queued-refresh";
import {
  beginEditorBufferOperation,
  captureEditorBufferState,
  detachEditorBuffer,
  editorBufferForModel,
  editorBuffersForPath,
  isEditorBufferStateCurrent,
  lockEditorBuffer,
  pathDerivedModelLanguage,
  restoreDetachedEditorBuffer,
  setLanguageDocumentAttacher,
  type EditorBuffer,
} from "./editor-buffers";
import {
  planWorkspaceEditModelLineages,
  type WorkspaceEditModelOutcome,
} from "./workspace-edit-model-lineages";
import {
  applyWorkspaceEditTransaction,
  registerPersistedWorkspaceEditRecoveries,
  retryWorkspaceEditRecoveries,
  replaceWorkspaceEditModel,
  type OpenWorkspaceEditTarget,
} from "./workspace-edit-transaction";
import { useWorkspaceStore } from "@/renderer/stores/workspace-store";
import { hasIpcErrorCode } from "./errors";
import { requestWorkspaceTrust } from "@/renderer/stores/workspace-trust-store";
import { canRetryWorkspaceTrustDocument } from "./workspace-trust-retry";

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
  bindingRetryTimer: number | null;
  connectionEpoch: number;
  disposed: boolean;
};

type LanguageDocumentCandidate = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  path: string;
  model: monaco.editor.ITextModel;
  activeHandle?: LanguageDocumentHandle;
  disposed: boolean;
};

const documents = new Map<string, LanguageDocument>();
const documentCandidates = new Map<string, LanguageDocumentCandidate>();
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
const codeActionMetadata = new WeakMap<
  monaco.languages.CodeAction,
  {
    document: LanguageDocument;
    generation: number;
    version: number;
    action: LanguageServerCodeAction;
    resolving: Promise<LanguageServerCodeAction> | null;
  }
>();
const preparedRenameServers = new Map<string, string>();
const DIAGNOSTIC_OWNER_PREFIX = "kosmos-language-server:";
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
const SIGNATURE_TRIGGER_CHARACTERS = ["(", ","];
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
const originalJsonMode = { ...monacoJson.jsonDefaults.modeConfiguration };
const originalCssMode = { ...monacoCss.cssDefaults.modeConfiguration };
const originalScssMode = { ...monacoCss.scssDefaults.modeConfiguration };
const originalLessMode = { ...monacoCss.lessDefaults.modeConfiguration };
const originalHtmlMode = { ...monacoHtml.htmlDefaults.modeConfiguration };
const languageServerStatuses = new Map<string, LanguageServerSnapshot>();
const activeServerInstallations = new Map<string, string | null>();
const registeredProviderLanguages = new Set<string>();
let suppressedLanguageFeatures = new Map<string, ReadonlySet<MonacoLanguageFeature>>();
let availableExternalLanguages = new Set<string>();
type DocumentSupport = {
  languages: Set<string>;
  formatters: FormatterSnapshot[];
};

let documentSupport: DocumentSupport = {
  languages: new Set(),
  formatters: [],
};
let catalogRetryTimer: number | null = null;
let nextGeneration = randomGenerationSeed();
let initialized = false;
const activeServerWorkspaceEdits = new Map<string, AbortController>();
let languageLocationOpener: ((
  workspaceId: WorkspaceId,
  path: string,
  selection: monaco.IRange,
) => Promise<boolean>) | null = null;

export function setLanguageLocationOpener(
  opener: typeof languageLocationOpener,
): void {
  languageLocationOpener = opener;
}

export function initializeLanguageClient(): void {
  setLanguageDocumentAttacher(attachLanguageDocument);
  if (initialized) {
    return;
  }
  initialized = true;

  window.kosmos.onServerNotification((notification) => {
    if (notification.event === "languageServerDiagnosticsChanged") {
      applyLanguageServerDiagnosticsChanged(notification);
    } else if (notification.event === "languageServerDiagnosticsResync") {
      void refreshDiagnosticsAfterReconnect();
    } else if (notification.event === "languageServerStatusChanged") {
      void getLanguageServerStatus({ serverId: notification.serverId })
        .then(applyLanguageServerStatus)
        .catch(() => scheduleCatalogRefresh());
    } else if (notification.event === "languageServerApplyEdit") {
      handleServerWorkspaceEdit(notification);
    } else if (notification.event === "languageServerApplyEditCancelled") {
      activeServerWorkspaceEdits
        .get(notification.token)
        ?.abort(new Error("Workspace edit application was cancelled by the server."));
    }
  });
  void window.kosmos.pendingServerApplyEdits().then((pending) => {
    pending.forEach(handleServerWorkspaceEdit);
  }).catch(() => {});
  void refreshPersistedWorkspaceEditRecoveries().then(retryWorkspaceEditRecoveries).catch(() => {});
  window.kosmos.onServerReconnected(() => {
    void refreshLanguageClientCatalog();
    void refreshDiagnosticsAfterReconnect();
    void refreshPersistedWorkspaceEditRecoveries()
      .then(retryWorkspaceEditRecoveries)
      .then(() => {
        window.dispatchEvent(new CustomEvent("kosmos:workspace-edit-applied"));
      })
      .catch(() => {});
  });
  monaco.editor.registerEditorOpener({
    async openCodeEditor(_source, resource, selectionOrPosition) {
      const location = kosmosResource(resource);
      if (!location) {
        return false;
      }
      const opener = languageLocationOpener;
      if (!opener) {
        return false;
      }
      const selection = selectionOrPosition
        ? "startLineNumber" in selectionOrPosition
          ? selectionOrPosition
          : {
              startLineNumber: selectionOrPosition.lineNumber,
              startColumn: selectionOrPosition.column,
              endLineNumber: selectionOrPosition.lineNumber,
              endColumn: selectionOrPosition.column,
            }
        : { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 1 };
      return opener(location.workspaceId, location.path, selection);
    },
  });
  monaco.editor.registerCommand("kosmos.applyCodeAction", async (_accessor, action: monaco.languages.CodeAction) => {
    await applyCodeAction(action);
  });
  void refreshLanguageClientCatalog();
}

async function refreshPersistedWorkspaceEditRecoveries(): Promise<void> {
  const recoveries = await listWorkspaceEditRecoveries();
  registerPersistedWorkspaceEditRecoveries(recoveries, persistedWorkspaceEditAdapter);
}

function persistedWorkspaceEditAdapter(recovery: WorkspaceEditRecovery) {
  const params = {
    transactionId: recovery.transactionId,
    authorization: recovery.authorization,
  };
  return {
    validate() {},
    preflight() {
      return null;
    },
    async commitClosed() {
      throw new Error("A recovered workspace edit cannot be committed again.");
    },
    async rollbackClosed() {
      await rollbackWorkspaceEdit(params);
    },
    async finish() {
      await finishWorkspaceEdit(params);
    },
    async acknowledge() {
      await acknowledgeWorkspaceEditCompletion(params);
    },
    finalize() {
      return finalizeWorkspaceEdit(params);
    },
    status() {
      return getWorkspaceEditStatus(params);
    },
    isRecoveryRequired: workspaceEditRecoveryRequired,
    isUnknownTransaction: workspaceEditUnknown,
    async reconcileUnknown() {
      await reconcileWorkspaceEditState();
    },
    async reconcileCompletion() {
      await reconcileWorkspaceEditState();
    },
  };
}

function workspaceEditRecoveryRequired(error: unknown): boolean {
  return ipcErrorHasCode(error, "workspace_edit.recovery_required");
}

function workspaceEditUnknown(error: unknown): boolean {
  return ipcErrorHasCode(error, "workspace_edit.expired") ||
    ipcErrorHasCode(error, "workspace_edit.invalid");
}

function ipcErrorHasCode(error: unknown, code: string): boolean {
  return Boolean(error && typeof error === "object" && "code" in error && error.code === code) ||
    (error instanceof Error && error.message.includes(code));
}

function handleServerWorkspaceEdit(
  notification: Extract<KosmosServerNotification, { event: "languageServerApplyEdit" }>,
): void {
  if (activeServerWorkspaceEdits.has(notification.token)) {
    return;
  }
  const cancellation = new AbortController();
  activeServerWorkspaceEdits.set(notification.token, cancellation);
  void applyStagedWorkspaceEdit(notification.edit, cancellation.signal, true)
    .then(
      () =>
        window.kosmos.acknowledgeServerApplyEdit(
          notification.id,
          notification.token,
          true,
        ),
      (error: unknown) =>
        window.kosmos.acknowledgeServerApplyEdit(
          notification.id,
          notification.token,
          false,
          error instanceof Error ? error.message : String(error),
        ),
    )
    .finally(() => activeServerWorkspaceEdits.delete(notification.token));
}

const requestCatalogRefresh = createQueuedRefresh(async () => {
  await Promise.all([listLanguageServers(), listFormatters()])
    .then(([snapshot, formatterSnapshot]) => {
      if (catalogRetryTimer !== null) {
        window.clearTimeout(catalogRetryTimer);
        catalogRetryTimer = null;
      }
      const languages = new Set(
        snapshot.servers
          .filter((server) => activeSelectedInstallation(server) !== null)
          .flatMap((server) => server.languageIds),
      );
      const installedFormatters = formatterSnapshot.formatters.filter(
        (formatter) =>
          formatter.installedVersion !== null && formatter.installationState !== "uninstalling",
      );
      for (const language of discoverProviderLanguages(registeredProviderLanguages, snapshot.servers)) {
        monaco.languages.registerHoverProvider(
          { language, scheme: "kosmos" },
          {
            async provideHover(model, position, token) {
              const document = documents.get(model.uri.toString());
              if (
                !document ||
                document.disposed ||
                token.isCancellationRequested ||
                !externalLanguageAvailable(document)
              ) {
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
              if (
                !document ||
                document.disposed ||
                token.isCancellationRequested ||
                !externalLanguageAvailable(document)
              ) {
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
                !externalLanguageAvailable(metadata.document) ||
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
                }, token);
                if (
                  metadata.document.disposed ||
                  !externalLanguageAvailable(metadata.document) ||
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
              if (
                !document ||
                document.disposed ||
                token.isCancellationRequested ||
                !externalLanguageAvailable(document)
              ) {
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
                  }, token);
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
                !externalLanguageAvailable(metadata.document) ||
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
                }, token);
                if (
                  metadata.document.disposed ||
                  !externalLanguageAvailable(metadata.document) ||
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
        registerReadOnlyLanguageProviders(language);
      }
      for (const server of snapshot.servers) {
        applyLanguageServerStatus(server);
      }
      documentSupport = { languages, formatters: installedFormatters };
      reconcileDocumentAttachments();
    })
    .catch(() => {
      scheduleCatalogRefresh();
    });
});

export function refreshLanguageClientCatalog(): Promise<void> {
  return requestCatalogRefresh();
}

function registerReadOnlyLanguageProviders(language: string): void {
  const selector = { language, scheme: "kosmos" };
  monaco.languages.registerSignatureHelpProvider(selector, {
    signatureHelpTriggerCharacters: SIGNATURE_TRIGGER_CHARACTERS,
    signatureHelpRetriggerCharacters: SIGNATURE_TRIGGER_CHARACTERS,
    async provideSignatureHelp(model, position, token) {
      const result = await requestCurrentDocumentFeature(model, token, (document, generation, version) =>
        getLanguageServerSignatureHelp({
          ...documentPositionParams(document, generation, version, position),
        }, token),
      );
      if (!result) {
        return null;
      }
      return {
        value: {
          signatures: result.signatures.map((signature) => ({
            label: signature.label,
            documentation: markupContent(signature.documentation),
            parameters: signature.parameters.map((parameter) => ({
              label: parameter.label,
              documentation: markupContent(parameter.documentation),
            })),
            activeParameter: signature.activeParameter ?? undefined,
          })),
          activeSignature: result.activeSignature ?? 0,
          activeParameter: result.activeParameter ?? 0,
        },
        dispose() {},
      };
    },
  });

  registerLocationProvider(selector, "definition");
  registerLocationProvider(selector, "declaration");
  registerLocationProvider(selector, "typeDefinition");
  registerLocationProvider(selector, "implementation");

  monaco.languages.registerReferenceProvider(selector, {
    async provideReferences(model, position, context, token) {
      const locations = await requestCurrentDocumentFeature(
        model,
        token,
        (document, generation, version) =>
          getLanguageServerReferences(
            {
              ...documentPositionParams(document, generation, version, position),
              includeDeclaration: context.includeDeclaration,
            },
            token,
          ),
      );
      return locations?.map(monacoLocation) ?? [];
    },
  });

  monaco.languages.registerDocumentSymbolProvider(selector, {
    displayName: "Kosmos language servers",
    async provideDocumentSymbols(model, token) {
      const symbols = await requestCurrentDocumentFeature(
        model,
        token,
        (document, generation, version) =>
          getLanguageServerDocumentSymbols(
            {
              workspaceId: document.workspaceId,
              path: document.path,
              generation,
              version,
            },
            token,
          ),
      );
      return symbols?.map(monacoDocumentSymbol) ?? [];
    },
  });
  monaco.languages.registerRenameProvider(selector, {
    async resolveRenameLocation(model, position, token) {
      const result = await requestCurrentDocumentFeature(
        model,
        token,
        (document, generation, version) =>
          prepareLanguageServerRename(
            documentPositionParams(document, generation, version, position),
            token,
          ),
      );
      if (!result) {
        return { range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column), text: "", rejectReason: "Rename is not available here." };
      }
      preparedRenameServers.set(renamePreparationKey(model, position), result.serverId);
      trimPreparedRenameServers();
      const defaultWord = model.getWordAtPosition(position);
      const range = result.range
        ? monacoRange(result.range)
        : defaultWord
          ? new monaco.Range(
              position.lineNumber,
              defaultWord.startColumn,
              position.lineNumber,
              defaultWord.endColumn,
            )
          : new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column);
      if (!result.range && !defaultWord) {
        return { range, text: "", rejectReason: "Rename is not available here." };
      }
      return {
        range,
        text: result.placeholder ?? model.getValueInRange(range),
      };
    },
    async provideRenameEdits(model, position, newName, token) {
      const document = documents.get(model.uri.toString());
      if (
        !document ||
        document.disposed ||
        token.isCancellationRequested ||
        !externalLanguageAvailable(document)
      ) {
        return { edits: [], rejectReason: "The document is no longer available." };
      }
      try {
        const staged = await afterPendingChanges(document, async () => {
          await ensureOpen(document);
          const generation = document.generation;
          const version = model.getVersionId();
          const serverId = preparedRenameServers.get(renamePreparationKey(model, position)) ?? null;
          const edit = await requestLanguageServerRename(
            {
              ...documentPositionParams(document, generation, version, position),
              newName,
              serverId,
            },
            token,
          );
          return { edit, generation, version };
        });
        if (
          token.isCancellationRequested ||
          !isCurrentLanguageResult(document, staged.generation, staged.version, false)
        ) {
          await finishWorkspaceEdit({
            transactionId: staged.edit.transactionId,
            authorization: staged.edit.authorization,
          });
          return { edits: [], rejectReason: "The document changed before rename could be applied." };
        }
        await applyStagedWorkspaceEdit(staged.edit);
        return { edits: [] };
      } catch (error) {
        return { edits: [], rejectReason: error instanceof Error ? error.message : "Rename failed." };
      }
    },
  });
  monaco.languages.registerCodeActionProvider(selector, {
    async provideCodeActions(model, range, context, token) {
      const result = await requestCurrentDocumentFeature(
        model,
        token,
        async (document, generation, version) => ({
          document,
          generation,
          version,
          actions: await getLanguageServerCodeActions(
            {
              workspaceId: document.workspaceId,
              path: document.path,
              generation,
              version,
              range: languageServerRange(range),
              context: codeActionContext(context),
            },
            token,
          ),
        }),
      );
      if (!result) {
        return { actions: [], dispose() {} };
      }
      const actions = result.actions.map((action) => {
        const item: monaco.languages.CodeAction = {
          title: action.title,
          kind: action.kind ?? undefined,
          isPreferred: action.isPreferred,
          disabled: action.disabledReason ? action.disabledReason : undefined,
          command: { id: "kosmos.applyCodeAction", title: action.title, arguments: [] },
        };
        item.command!.arguments = [item];
        codeActionMetadata.set(item, {
          document: result.document,
          generation: result.generation,
          version: result.version,
          action,
          resolving: null,
        });
        return item;
      });
      return { actions, dispose() {} };
    },
    async resolveCodeAction(action, token) {
      const metadata = codeActionMetadata.get(action);
      if (!metadata || token.isCancellationRequested) {
        return action;
      }
      try {
        const resolved = await resolveCodeActionMetadata(metadata, token);
        if (!isCurrentLanguageResult(metadata.document, metadata.generation, metadata.version, token.isCancellationRequested)) {
          return action;
        }
        metadata.action = resolved;
        action.title = resolved.title;
        action.kind = resolved.kind ?? undefined;
        action.isPreferred = resolved.isPreferred;
        action.disabled = resolved.disabledReason ?? undefined;
      } catch {
        // Monaco can still invoke the unresolved action.
      }
      return action;
    },
  });
}

async function applyCodeAction(action: monaco.languages.CodeAction): Promise<void> {
  const metadata = codeActionMetadata.get(action);
  if (!metadata || metadata.document.disposed || !externalLanguageAvailable(metadata.document)) {
    throw new Error("The code action is no longer available.");
  }
  let serverAction = metadata.action;
  if (serverAction.resolveSupported) {
    serverAction = await resolveCodeActionMetadata(metadata);
  }
  if (!isCurrentLanguageResult(metadata.document, metadata.generation, metadata.version, false)) {
    throw new Error("The document changed before the code action could be applied.");
  }
  const staged = await stageLanguageServerCodeAction(serverAction);
  if (staged) {
    await applyStagedWorkspaceEdit(staged);
  }
  if (serverAction.commandAuthorization) {
    await afterPendingChanges(metadata.document, () =>
      executeLanguageServerCommand({
        workspaceId: metadata.document.workspaceId,
        path: metadata.document.path,
        generation: metadata.document.generation,
        version: metadata.document.model.getVersionId(),
        serverId: serverAction.serverId,
        authorization: serverAction.commandAuthorization!,
      }),
    );
  }
}

async function resolveCodeActionMetadata(
  metadata: NonNullable<ReturnType<typeof codeActionMetadata.get>>,
  token?: monaco.CancellationToken,
): Promise<LanguageServerCodeAction> {
  if (!metadata.action.resolveSupported) {
    return metadata.action;
  }
  metadata.resolving ??= resolveLanguageServerCodeAction(
    {
      workspaceId: metadata.document.workspaceId,
      path: metadata.document.path,
      generation: metadata.generation,
      version: metadata.version,
      serverId: metadata.action.serverId,
      actionId: metadata.action.actionId,
      raw: metadata.action.raw,
    },
    token,
  );
  try {
    metadata.action = await metadata.resolving;
    return metadata.action;
  } finally {
    metadata.resolving = null;
  }
}

export async function applyStagedWorkspaceEdit(
  edit: StagedWorkspaceEdit,
  signal?: AbortSignal,
  deferCompletionAcknowledgement = false,
): Promise<void> {
  const validateDocument = (
    document: StagedWorkspaceEdit["documents"][number],
    path = document.path,
    strictGeneration = true,
  ): void => {
    const uri = locationUri({ ...document, path });
    const model = monaco.editor.getModel(uri);
    if (document.generation === null || document.version === null) {
      if (model) {
        throw new Error(`Workspace edit target ${document.path} opened after validation.`);
      }
      return;
    }
    const languageDocument = documents.get(uri.toString());
    if (
      !model ||
      !languageDocument ||
      languageDocument.disposed ||
      (strictGeneration &&
        (languageDocument.generation !== document.generation ||
          model.getVersionId() !== document.version)) ||
      model.getValue() !== document.originalText
    ) {
      throw new Error(`Workspace edit target ${document.path} changed before it could be applied.`);
    }
  };
  await applyWorkspaceEditTransaction(edit, {
    validate: validateDocument,
    preflight() {
      return null;
    },
    preflightTargets() {
      return preflightWorkspaceEditModelTargets(edit, validateDocument);
    },
    async commitClosed(transactionId) {
      await commitWorkspaceEdit({ transactionId, authorization: edit.authorization });
    },
    async rollbackClosed(transactionId) {
      await rollbackWorkspaceEdit({ transactionId, authorization: edit.authorization });
    },
    async finish(transactionId) {
      await finishWorkspaceEdit({ transactionId, authorization: edit.authorization });
    },
    async acknowledge(transactionId) {
      await acknowledgeWorkspaceEditCompletion({
        transactionId,
        authorization: edit.authorization,
      });
    },
    async finalize(transactionId) {
      return finalizeWorkspaceEdit({ transactionId, authorization: edit.authorization });
    },
    status(transactionId) {
      return getWorkspaceEditStatus({ transactionId, authorization: edit.authorization });
    },
    isRecoveryRequired: workspaceEditRecoveryRequired,
    isUnknownTransaction: workspaceEditUnknown,
    async reconcileUnknown() {
      await reconcileWorkspaceEditState();
    },
    async reconcileCompletion() {
      await reconcileWorkspaceEditState();
    },
  }, signal, deferCompletionAcknowledgement);
  window.dispatchEvent(new CustomEvent("kosmos:workspace-edit-applied"));
}

type WorkspaceEditBufferOutcome = WorkspaceEditModelOutcome<EditorBuffer>;

type PreparedWorkspaceEditBuffer = {
  outcome: WorkspaceEditBufferOutcome;
  state: ReturnType<typeof captureEditorBufferState>;
  languageDocument: LanguageDocument | null;
  generation: number | null;
  editors: Array<{
    editor: monaco.editor.ICodeEditor;
    viewState: monaco.editor.ICodeEditorViewState | null;
  }>;
  detached: boolean;
  bound: boolean;
  installedModel: monaco.editor.ITextModel | null;
  originalModel: monaco.editor.ITextModel;
};

function preflightWorkspaceEditModelTargets(
  edit: StagedWorkspaceEdit,
  validateDocument: (
    document: StagedWorkspaceEdit["documents"][number],
    path?: string,
    strictGeneration?: boolean,
  ) => void,
): OpenWorkspaceEditTarget[] {
  const buffers = collectWorkspaceEditBuffers(edit);
  for (const operation of edit.operations) {
    if (operation.kind !== "textDocument") continue;
    const document = edit.documents[operation.document];
    if (document && document.generation !== null && document.version !== null) {
      validateDocument(document, document.originalPath);
    }
  }
  const outcomes = planWorkspaceEditModelLineages(
    edit,
    [...buffers].map((buffer) => ({
      workspaceId: buffer.workspaceId,
      path: buffer.path,
      content: buffer.model.getValue(),
      savedContent: buffer.savedContent,
      value: buffer,
    })),
  );
  if (outcomes.length === 0) return [];

  const prepared = outcomes.map<PreparedWorkspaceEditBuffer>((outcome) => {
    const state = captureEditorBufferState(outcome.value);
    const languageDocument = documents.get(state.model.uri.toString()) ?? null;
    return {
      outcome,
      state,
      languageDocument,
      generation: languageDocument?.generation ?? null,
      editors: [],
      detached: false,
      bound: false,
      installedModel: null,
      originalModel: state.model,
    };
  });
  const releases = prepared.map(({ outcome }) =>
    lockEditorBuffer(outcome.value, edit.transactionId)
  );
  return [workspaceEditModelTarget(prepared, releases)];
}

function collectWorkspaceEditBuffers(edit: StagedWorkspaceEdit): Set<EditorBuffer> {
  const buffers = new Set<EditorBuffer>();
  const addPath = (workspaceId: number, path: string) => {
    for (const buffer of editorBuffersForPath(workspaceId, path)) buffers.add(buffer);
  };
  for (const operation of edit.operations) {
    if (operation.kind === "textDocument") {
      const document = edit.documents[operation.document];
      if (document && document.generation !== null && document.version !== null) {
        addPath(document.workspaceId, document.originalPath);
      }
    } else if (operation.kind === "renameFile") {
      addPath(operation.workspaceId, operation.oldPath);
      addPath(operation.workspaceId, operation.newPath);
    } else {
      addPath(operation.workspaceId, operation.path);
    }
  }
  return buffers;
}

function workspaceEditModelTarget(
  prepared: PreparedWorkspaceEditBuffer[],
  releases: Array<() => void>,
): OpenWorkspaceEditTarget {
  let completed = false;
  return {
    validate() {
      for (const current of prepared) {
        const languageDocument = documents.get(current.state.model.uri.toString()) ?? null;
        if (
          !isEditorBufferStateCurrent(current.state) ||
          languageDocument !== current.languageDocument ||
          (languageDocument?.generation ?? null) !== current.generation ||
          languageDocument?.disposed === true
        ) {
          throw new Error(
            `Open workspace edit target ${current.state.path} changed before application.`,
          );
        }
      }
    },
    apply() {
      prepareWorkspaceEditViews(prepared);
      for (const current of prepared) {
        detachEditorBuffer(current.outcome.value);
        current.detached = true;
      }
      disposeConflictingOriginalModels(prepared);
      for (const current of prepared) installWorkspaceEditOutcome(current);
    },
    undo() {
      if (completed) return;
      validateAppliedWorkspaceEditModels(prepared);
      for (const current of prepared) {
        if (!current.detached) continue;
        if (current.bound) {
          detachEditorBuffer(current.outcome.value);
          current.bound = false;
        }
        current.installedModel?.dispose();
        current.installedModel = null;
      }
      for (const current of prepared) restoreWorkspaceEditBuffer(current);
    },
    complete() {
      completed = true;
      for (const current of prepared) {
        if (current.outcome.finalPath === null || current.installedModel !== null) {
          current.originalModel.dispose();
        }
      }
    },
    release() {
      for (const release of releases) release();
    },
  };
}

function prepareWorkspaceEditViews(prepared: PreparedWorkspaceEditBuffer[]): void {
  for (const current of prepared) {
    current.editors = monaco.editor
      .getEditors()
      .filter((editor) => editor.getModel() === current.originalModel)
      .map((editor) => ({ editor, viewState: editor.saveViewState() }));
    for (const editor of current.editors) editor.editor.setModel(null);
  }
}

function disposeConflictingOriginalModels(prepared: PreparedWorkspaceEditBuffer[]): void {
  const finalOwners = new Map(
    prepared
      .filter(({ outcome }) => outcome.finalPath !== null)
      .map((current) => [
        `${current.outcome.workspaceId}:${current.outcome.finalPath}`,
        current,
      ]),
  );
  for (const current of prepared) {
    const owner = finalOwners.get(`${current.outcome.workspaceId}:${current.state.path}`);
    if (owner && owner !== current) current.originalModel.dispose();
  }
}

function installWorkspaceEditOutcome(current: PreparedWorkspaceEditBuffer): void {
  const { outcome } = current;
  if (outcome.finalPath === null) return;
  let model = current.originalModel;
  if (outcome.finalPath !== current.state.path || model.isDisposed()) {
    model = monaco.editor.createModel(
      outcome.finalContent,
      pathDerivedModelLanguage(),
      locationUri({ workspaceId: outcome.workspaceId, path: outcome.finalPath }),
    );
    current.installedModel = model;
  } else if (model.getValue() !== outcome.finalContent) {
    // setValue intentionally clears per-model undo. One model cannot undo part of a transaction.
    replaceWorkspaceEditModel(model, outcome.finalContent);
  }
  restoreDetachedEditorBuffer(outcome.value, outcome.finalPath, model);
  current.bound = true;
  for (const editor of current.editors) {
    editor.editor.setModel(model);
    if (editor.viewState) editor.editor.restoreViewState(editor.viewState);
  }
}

function validateAppliedWorkspaceEditModels(prepared: PreparedWorkspaceEditBuffer[]): void {
  for (const current of prepared) {
    if (!current.bound) continue;
    const model = current.installedModel ?? current.originalModel;
    if (model.isDisposed() || model.getValue() !== current.outcome.finalContent) {
      throw new Error(
        `Monaco target ${current.outcome.finalPath} changed after application; recovery will not overwrite it.`,
      );
    }
  }
}

function restoreWorkspaceEditBuffer(current: PreparedWorkspaceEditBuffer): void {
  if (!current.detached) return;
  let model = current.originalModel;
  if (model.isDisposed()) {
    model = monaco.editor.createModel(
      current.state.content,
      pathDerivedModelLanguage(),
      locationUri({
        workspaceId: current.outcome.workspaceId,
        path: current.state.path,
      }),
    );
  } else if (model.getValue() !== current.state.content) {
    replaceWorkspaceEditModel(model, current.state.content);
  }
  restoreDetachedEditorBuffer(current.outcome.value, current.state.path, model);
  for (const editor of current.editors) {
    editor.editor.setModel(model);
    if (editor.viewState) editor.editor.restoreViewState(editor.viewState);
  }
  current.detached = false;
}

async function reconcileWorkspaceEditState(): Promise<void> {
  await useWorkspaceStore.getState().refreshWorkspaces();
  window.dispatchEvent(new CustomEvent("kosmos:workspace-edit-applied"));
}

function renamePreparationKey(model: monaco.editor.ITextModel, position: monaco.Position): string {
  return `${model.uri.toString()}|${model.getVersionId()}|${position.lineNumber}:${position.column}`;
}

function trimPreparedRenameServers(): void {
  while (preparedRenameServers.size > 256) {
    const oldest = preparedRenameServers.keys().next().value;
    if (oldest === undefined) {
      return;
    }
    preparedRenameServers.delete(oldest);
  }
}

function languageServerRange(range: monaco.IRange): LanguageServerRange {
  return {
    start: { line: range.startLineNumber - 1, character: range.startColumn - 1 },
    end: { line: range.endLineNumber - 1, character: range.endColumn - 1 },
  };
}

function codeActionContext(context: monaco.languages.CodeActionContext): unknown {
  return {
    diagnostics: context.markers.map((marker) => ({
      range: languageServerRange(marker),
      severity:
        marker.severity === monaco.MarkerSeverity.Error
          ? 1
          : marker.severity === monaco.MarkerSeverity.Warning
            ? 2
            : marker.severity === monaco.MarkerSeverity.Info
              ? 3
              : 4,
      message: marker.message,
      source: marker.source,
      code: typeof marker.code === "object" ? marker.code.value : marker.code,
    })),
    only: context.only,
    triggerKind: context.trigger,
  };
}

type LocationFeature = "definition" | "declaration" | "typeDefinition" | "implementation";

function registerLocationProvider(
  selector: { language: string; scheme: string },
  feature: LocationFeature,
): void {
  const provide = async (
    model: monaco.editor.ITextModel,
    position: monaco.Position,
    token: monaco.CancellationToken,
  ) => {
    const request = {
      definition: getLanguageServerDefinitions,
      declaration: getLanguageServerDeclarations,
      typeDefinition: getLanguageServerTypeDefinitions,
      implementation: getLanguageServerImplementations,
    }[feature];
    const locations = await requestCurrentDocumentFeature(
      model,
      token,
      (document, generation, version) =>
        request(documentPositionParams(document, generation, version, position), token),
    );
    return locations?.map(monacoLocationLink) ?? [];
  };
  switch (feature) {
    case "definition":
      monaco.languages.registerDefinitionProvider(selector, { provideDefinition: provide });
      break;
    case "declaration":
      monaco.languages.registerDeclarationProvider(selector, { provideDeclaration: provide });
      break;
    case "typeDefinition":
      monaco.languages.registerTypeDefinitionProvider(selector, { provideTypeDefinition: provide });
      break;
    case "implementation":
      monaco.languages.registerImplementationProvider(selector, { provideImplementation: provide });
      break;
  }
}

async function requestCurrentDocumentFeature<T>(
  model: monaco.editor.ITextModel,
  token: monaco.CancellationToken,
  request: (
    document: LanguageDocument,
    generation: number,
    version: number,
  ) => Promise<T>,
): Promise<T | null> {
  const document = documents.get(model.uri.toString());
  if (
    !document ||
    document.disposed ||
    token.isCancellationRequested ||
    !availableExternalLanguages.has(model.getLanguageId())
  ) {
    return null;
  }
  try {
    return await afterPendingChanges(document, async () => {
      await ensureOpen(document);
      const generation = document.generation;
      const version = model.getVersionId();
      const value = await request(document, generation, version);
      return isCurrentLanguageResult(
        document,
        generation,
        version,
        token.isCancellationRequested,
      )
        ? value
        : null;
    });
  } catch {
    return null;
  }
}

function documentPositionParams(
  document: LanguageDocument,
  generation: number,
  version: number,
  position: monaco.Position,
) {
  return {
    workspaceId: document.workspaceId,
    path: document.path,
    generation,
    version,
    position: {
      line: position.lineNumber - 1,
      character: position.column - 1,
    },
  };
}

function monacoLocation(location: LanguageServerLocation): monaco.languages.Location {
  return { uri: locationUri(location), range: monacoRange(location.selectionRange) };
}

function monacoLocationLink(location: LanguageServerLocation): monaco.languages.LocationLink {
  return {
    uri: locationUri(location),
    range: monacoRange(location.range),
    targetSelectionRange: monacoRange(location.selectionRange),
  };
}

function locationUri(location: Pick<LanguageServerLocation, "workspaceId" | "path">): monaco.Uri {
  return monaco.Uri.from({
    scheme: "kosmos",
    authority: `workspace-${location.workspaceId}`,
    path: `/${location.path}`,
  });
}

function kosmosResource(resource: monaco.Uri): { workspaceId: WorkspaceId; path: string } | null {
  const match = /^workspace-(\d+)$/.exec(resource.authority);
  const path = resource.path.replace(/^\//, "");
  if (
    resource.scheme !== "kosmos" ||
    !match ||
    !path ||
    path.split("/").some((component) => component === "" || component === "." || component === "..")
  ) {
    return null;
  }
  const workspaceId = Number(match[1]);
  return Number.isSafeInteger(workspaceId) ? { workspaceId, path } : null;
}

function monacoDocumentSymbol(symbol: LanguageServerDocumentSymbol): monaco.languages.DocumentSymbol {
  return {
    name: symbol.name,
    detail: symbol.detail ?? "",
    kind: symbolKind(symbol.kind),
    tags: symbol.deprecated ? [monaco.languages.SymbolTag.Deprecated] : [],
    range: monacoRange(symbol.range),
    selectionRange: monacoRange(symbol.selectionRange),
    children: symbol.children.map(monacoDocumentSymbol),
  };
}

function symbolKind(kind: number): monaco.languages.SymbolKind {
  return kind >= 1 && kind <= 26
    ? (kind - 1) as monaco.languages.SymbolKind
    : monaco.languages.SymbolKind.Variable;
}

function markupContent(
  content: { kind: "plainText" | "markdown"; value: string } | null,
): string | monaco.IMarkdownString | undefined {
  if (!content) {
    return undefined;
  }
  return content.kind === "markdown"
    ? { value: content.value, isTrusted: false, supportHtml: false }
    : content.value;
}

function scheduleCatalogRefresh(): void {
  if (catalogRetryTimer !== null) {
    return;
  }
  catalogRetryTimer = window.setTimeout(() => {
    catalogRetryTimer = null;
    void refreshLanguageClientCatalog();
  }, BINDING_RETRY_DELAY_MS);
}

export function applyLanguageServerStatus(server: LanguageServerSnapshot): void {
  languageServerStatuses.set(server.id, server);
  const activeInstallation = activeSelectedInstallation(server);
  const previousInstallation = activeServerInstallations.get(server.id) ?? null;
  activeServerInstallations.set(server.id, activeInstallation);
  availableExternalLanguages = activeExternalLanguages(languageServerStatuses.values());
  synchronizeMonacoLanguageFeatures();
  if (
    activeInstallation === null ||
    server.runtimeState === "crashed" ||
    server.runtimeState === "restarting"
  ) {
    for (const document of documents.values()) {
      if (server.languageIds.includes(document.model.getLanguageId())) {
        monaco.editor.setModelMarkers(document.model, diagnosticOwner(server.id), []);
      }
    }
  }
  if (server.languageIds.some((language) => !registeredProviderLanguages.has(language))) {
    void refreshLanguageClientCatalog();
  }
  if (activeInstallation === previousInstallation) {
    return;
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
    if (activeInstallation !== null) {
      document.opened = false;
      enqueue(document, () => ensureOpen(document));
    } else {
      document.opened = false;
      clearLanguageServerMarkers(document.model);
      enqueue(document, () => ensureOpen(document));
    }
  }
}

export async function formatLanguageDocument(
  editor: monaco.editor.IStandaloneCodeEditor,
  cancellation?: monaco.CancellationToken,
): Promise<boolean> {
  const model = editor.getModel();
  if (!model) {
    return false;
  }
  const document = documents.get(model.uri.toString());
  if (!document || document.disposed) {
    throw new Error("No formatter or language server is available for this document.");
  }
  const buffer = editorBufferForModel(model);
  const operation = buffer ? beginEditorBufferOperation(buffer, model) : null;
  let result;
  try {
    result = await afterPendingChanges(document, async () => {
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
      }, cancellation);
      return { edits, generation, version };
    });
  } catch (error) {
    if (isRequestCancelledError(error)) {
      return false;
    }
    throw error;
  }
  if (
    !result ||
    cancellation?.isCancellationRequested ||
    document.disposed ||
    document.generation !== result.generation ||
    model.getVersionId() !== result.version ||
    operation?.isCurrent() === false
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
  if (!applied) {
    throw new Error("Formatting edits could not be applied to the current document.");
  }
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
  const key = model.uri.toString();
  const candidate: LanguageDocumentCandidate = {
    workspaceId,
    tabId,
    path,
    model,
    disposed: false,
  };
  documentCandidates.set(key, candidate);
  attachCandidate(candidate);
  return {
    dispose() {
      candidate.disposed = true;
      candidate.activeHandle?.dispose();
      if (documentCandidates.get(key) === candidate) {
        documentCandidates.delete(key);
      }
    },
  };
}

function reconcileDocumentAttachments(): void {
  for (const candidate of documentCandidates.values()) {
    attachCandidate(candidate);
  }
}

function attachCandidate(candidate: LanguageDocumentCandidate): void {
  if (candidate.disposed) {
    return;
  }
  const supported = documentIsSupported(
    documentSupport.languages,
    documentSupport.formatters,
    candidate.model.getLanguageId(),
    candidate.path,
  );
  const action = documentAttachmentAction(Boolean(candidate.activeHandle), supported);
  if (action === "detach") {
    candidate.activeHandle?.dispose();
    candidate.activeHandle = undefined;
    return;
  }
  if (action === "keep") {
    return;
  }
  candidate.activeHandle = attachSupportedLanguageDocument(
    candidate.workspaceId,
    candidate.tabId,
    candidate.path,
    candidate.model,
  );
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
      if (document.bindingRetryTimer !== null) {
        window.clearTimeout(document.bindingRetryTimer);
      }
      documents.delete(model.uri.toString());
      clearLanguageServerMarkers(model);
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
    if (!(await requestWorkspaceTrustAfterError(document.workspaceId, caughtError))) {
      if (isTransientBindingError(caughtError)) {
        scheduleBindingRetry(document, connectionEpoch);
      }
      throw caughtError;
    }
    if (!canRetryWorkspaceTrustDocument(document, connectionEpoch)) {
      return;
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
  void refreshDocumentDiagnostics(document);
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

function requestWorkspaceTrustAfterError(workspaceId: WorkspaceId, error: unknown): Promise<boolean> {
  if (!hasIpcErrorCode(error, "language_servers.workspace_not_trusted")) {
    return Promise.resolve(false);
  }

  return requestWorkspaceTrust(workspaceId, () =>
    trustLanguageServerWorkspace({ workspaceId }),
  ).then((decision) => decision === "trust");
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
    clearLanguageServerMarkers(document.model);
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
  } catch {
    document.opened = false;
    await ensureOpen(document);
  }
}

export function applyLanguageServerDiagnosticsChanged(
  event: LanguageServerDiagnosticsChanged,
): boolean {
  const document = [...documents.values()].find(
    (candidate) =>
      candidate.workspaceId === event.workspaceId && candidate.path === event.path,
  );
  if (!document || !isCurrentDiagnostics(event, document)) {
    return false;
  }
  monaco.editor.setModelMarkers(
    document.model,
    diagnosticOwner(event.serverId),
    event.diagnostics.map(diagnosticMarker),
  );
  return true;
}

async function refreshDiagnosticsAfterReconnect(): Promise<void> {
  for (const document of documents.values()) {
    await document.queue;
    await refreshDocumentDiagnostics(document);
  }
}

async function refreshDocumentDiagnostics(document: LanguageDocument): Promise<void> {
  const generation = document.generation;
  const version = document.syncedVersion;
  if (!isCurrentDiagnostics({ generation, version }, document)) {
    return;
  }
  try {
    const diagnostics = await getLanguageServerDiagnostics({
      workspaceId: document.workspaceId,
      path: document.path,
      generation,
      version,
    });
    if (diagnostics && isCurrentDiagnostics({ generation, version }, document)) {
      for (const snapshot of diagnostics) {
        monaco.editor.setModelMarkers(
          document.model,
          diagnosticOwner(snapshot.serverId),
          snapshot.diagnostics.map(diagnosticMarker),
        );
      }
    }
  } catch {
    // A later pushed diagnostic or reconnect refresh will recover the view.
  }
}

function diagnosticMarker(diagnostic: {
  range: LanguageServerRange;
  severity: "error" | "warning" | "information" | "hint" | null;
  message: string;
  source: string | null;
  code: string | null;
}): monaco.editor.IMarkerData {
  return {
    startLineNumber: diagnostic.range.start.line + 1,
    startColumn: diagnostic.range.start.character + 1,
    endLineNumber: diagnostic.range.end.line + 1,
    endColumn: diagnostic.range.end.character + 1,
    severity: markerSeverity(diagnostic.severity),
    message: diagnostic.message,
    source: diagnostic.source ?? undefined,
    code: diagnostic.code ?? undefined,
  };
}

function diagnosticOwner(serverId: string): string {
  return `${DIAGNOSTIC_OWNER_PREFIX}${serverId}`;
}

function clearLanguageServerMarkers(model: monaco.editor.ITextModel): void {
  for (const serverId of languageServerStatuses.keys()) {
    monaco.editor.setModelMarkers(model, diagnosticOwner(serverId), []);
  }
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
  if (suppressedLanguageFeatures.get(language)?.has("diagnostics")) {
    monaco.editor.setModelMarkers(model, language, []);
  }
}

function externalLanguageAvailable(document: LanguageDocument): boolean {
  return availableExternalLanguages.has(document.model.getLanguageId());
}

function synchronizeMonacoLanguageFeatures(): void {
  const nextFeatures = activePrimaryFeatures(languageServerStatuses.values());
  if (languageFeatureMapsEqual(suppressedLanguageFeatures, nextFeatures)) {
    return;
  }

  configureMode(
    monacoTypeScript.typescriptDefaults,
    originalTypeScriptMode,
    nextFeatures.get("typescript"),
  );
  configureMode(
    monacoTypeScript.javascriptDefaults,
    originalJavaScriptMode,
    nextFeatures.get("javascript"),
  );
  configureMode(monacoJson.jsonDefaults, originalJsonMode, nextFeatures.get("json"));
  configureMode(monacoCss.cssDefaults, originalCssMode, nextFeatures.get("css"));
  configureMode(monacoCss.scssDefaults, originalScssMode, nextFeatures.get("scss"));
  configureMode(monacoCss.lessDefaults, originalLessMode, nextFeatures.get("less"));
  configureMode(monacoHtml.htmlDefaults, originalHtmlMode, nextFeatures.get("html"));

  const typescriptDiagnostics = nextFeatures.get("typescript")?.has("diagnostics") === true;
  const javascriptDiagnostics = nextFeatures.get("javascript")?.has("diagnostics") === true;
  monacoTypeScript.typescriptDefaults.setDiagnosticsOptions(
    typescriptDiagnostics
      ? disabledTypeScriptDiagnostics(originalTypeScriptDiagnostics)
      : originalTypeScriptDiagnostics,
  );
  monacoTypeScript.javascriptDefaults.setDiagnosticsOptions(
    javascriptDiagnostics
      ? disabledTypeScriptDiagnostics(originalJavaScriptDiagnostics)
      : originalJavaScriptDiagnostics,
  );

  suppressedLanguageFeatures = nextFeatures;
  for (const document of documents.values()) {
    activateExternalLanguageFeatures(document.model);
  }
}

function configureMode<T extends object>(
  defaults: { setModeConfiguration(configuration: T): void },
  original: T,
  features: ReadonlySet<MonacoLanguageFeature> | undefined,
): void {
  defaults.setModeConfiguration({
    ...original,
    ...(features?.has("completionItems") ? { completionItems: false } : {}),
    ...(features?.has("hovers") ? { hovers: false } : {}),
    ...(features?.has("signatureHelp") ? { signatureHelp: false } : {}),
    ...(features?.has("definitions") ? { definitions: false } : {}),
    ...(features?.has("references") ? { references: false } : {}),
    ...(features?.has("documentSymbols") ? { documentSymbols: false } : {}),
    ...(features?.has("rename") ? { rename: false } : {}),
    ...(features?.has("codeActions") ? { codeActions: false } : {}),
    ...(features?.has("diagnostics") ? { diagnostics: false } : {}),
    ...(features?.has("colors") ? { colors: false } : {}),
    ...(features?.has("documentFormattingEdits")
      ? { documentFormattingEdits: false }
      : {}),
  });
}

function disabledTypeScriptDiagnostics<T extends object>(original: T): T {
  return {
    ...original,
    noSemanticValidation: true,
    noSyntaxValidation: true,
    noSuggestionDiagnostics: true,
  };
}

function languageFeatureMapsEqual(
  left: Map<string, ReadonlySet<MonacoLanguageFeature>>,
  right: Map<string, ReadonlySet<MonacoLanguageFeature>>,
): boolean {
  if (left.size !== right.size) {
    return false;
  }
  for (const [language, features] of left) {
    const other = right.get(language);
    if (!other || features.size !== other.size || [...features].some((feature) => !other.has(feature))) {
      return false;
    }
  }
  return true;
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
    }, token);
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
  }, token);
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
  return [
    "language_servers.worker_busy",
    "language_servers.worker_unavailable",
    "language_servers.server_start_failed",
    "language_servers.server_exited",
    "language_servers.request_timeout",
    "language_servers.runtime_unavailable",
  ].some((code) => hasIpcErrorCode(error, code));
}

function enqueue(document: LanguageDocument, operation: () => Promise<unknown>): void {
  document.queue = document.queue.then(operation).then(
    () => undefined,
    () => undefined,
  );
}

async function afterPendingChanges<T>(
  document: LanguageDocument,
  operation: () => Promise<T>,
): Promise<T> {
  await document.queue;
  return operation();
}
