export type LanguageServerInstallationState =
  | "notInstalled"
  | "installing"
  | "installed"
  | "uninstalling"
  | "failed";

export type LanguageServerRuntimeState =
  | "inactive"
  | "restarting"
  | "running"
  | "degraded"
  | "crashed";

export type LanguageServerFailure = {
  code: string;
  message: string;
};

export type LanguageServerLog = {
  kind: "stderr" | "runtime";
  message: string;
};

export type LanguageServerSnapshot = {
  id: string;
  name: string;
  description: string;
  languages: string[];
  languageIds: string[];
  catalogVersion: string;
  selectedVersion: string | null;
  installedVersion: string | null;
  installationState: LanguageServerInstallationState;
  lastError: LanguageServerFailure | null;
  runtimeState: LanguageServerRuntimeState;
  sessionCount: number;
  workspaceCount: number;
  runtimeError: LanguageServerFailure | null;
  logs: LanguageServerLog[];
  supported: boolean;
};

export type LanguageServerListSnapshot = {
  servers: LanguageServerSnapshot[];
};

export type LanguageServerParams = {
  serverId: string;
};

export type LanguageServerPosition = {
  line: number;
  character: number;
};

export type LanguageServerRange = {
  start: LanguageServerPosition;
  end: LanguageServerPosition;
};

export type LanguageServerChange = {
  range: LanguageServerRange;
  text: string;
};

export type OpenLanguageServerDocumentParams = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  languageId: string;
  generation: number;
  version: number;
  text: string;
};

export type ChangeLanguageServerDocumentParams = {
  workspaceId: WorkspaceId;
  path: string;
  generation: number;
  version: number;
  changes: LanguageServerChange[];
  text: string;
};

export type CloseLanguageServerDocumentParams = {
  workspaceId: WorkspaceId;
  path: string;
  generation: number;
};

export type SaveLanguageServerDocumentParams = CloseLanguageServerDocumentParams & {
  version: number;
  text: string;
};

export type LanguageServerHoverParams = CloseLanguageServerDocumentParams & {
  version: number;
  position: LanguageServerPosition;
};

export type LanguageServerDiagnosticsParams = CloseLanguageServerDocumentParams & {
  version: number;
};

export type LanguageServerHover = {
  contents: Array<{ kind: "plainText" | "markdown"; value: string }>;
  range: LanguageServerRange | null;
};

export type LanguageServerSignatureHelp = {
  signatures: LanguageServerSignatureInformation[];
  activeSignature: number | null;
  activeParameter: number | null;
};

export type LanguageServerSignatureInformation = {
  label: string;
  documentation: { kind: "plainText" | "markdown"; value: string } | null;
  parameters: Array<{
    label: string | [number, number];
    documentation: { kind: "plainText" | "markdown"; value: string } | null;
  }>;
  activeParameter: number | null;
};

export type LanguageServerLocation = {
  workspaceId: WorkspaceId;
  path: string;
  range: LanguageServerRange;
  selectionRange: LanguageServerRange;
};

export type LanguageServerReferencesParams = LanguageServerHoverParams & {
  includeDeclaration: boolean;
};

export type LanguageServerDocumentSymbol = {
  name: string;
  detail: string | null;
  kind: number;
  deprecated: boolean;
  range: LanguageServerRange;
  selectionRange: LanguageServerRange;
  children: LanguageServerDocumentSymbol[];
};

export type LanguageServerWorkspaceSymbolsParams = {
  query: string;
};

export type LanguageServerWorkspaceSymbol = {
  serverId: string;
  workspaceId: WorkspaceId;
  name: string;
  kind: number;
  containerName: string | null;
  deprecated: boolean;
  location: LanguageServerLocation | null;
  raw: unknown;
  resolveSupported: boolean;
};

export type ResolveLanguageServerWorkspaceSymbolParams = {
  serverId: string;
  workspaceId: WorkspaceId;
  raw: unknown;
};

export type LanguageServerDiagnostic = {
  range: LanguageServerRange;
  severity: "error" | "warning" | "information" | "hint" | null;
  message: string;
  source: string | null;
  code: string | null;
};

export type LanguageServerDiagnosticSnapshot = {
  serverId: string;
  diagnostics: LanguageServerDiagnostic[];
};

export type LanguageServerDiagnosticsChanged = {
  workspaceId: WorkspaceId;
  path: string;
  serverId: string;
  generation: number;
  version: number;
  diagnostics: LanguageServerDiagnostic[];
};

export type LanguageServerCompletionParams = CloseLanguageServerDocumentParams & {
  version: number;
  position: LanguageServerPosition;
  triggerKind: number;
  triggerCharacter: string | null;
  filter: string;
};

export type ResolveLanguageServerCompletionParams = CloseLanguageServerDocumentParams & {
  version: number;
  serverId: string;
  raw: unknown;
};

export type LanguageServerCompletionList = {
  items: LanguageServerCompletionItem[];
  isIncomplete: boolean;
};

export type LanguageServerCompletionItem = {
  serverId: string;
  label: string;
  labelDetail: string | null;
  labelDescription: string | null;
  kind: number | null;
  detail: string | null;
  documentation: { kind: "plainText" | "markdown"; value: string } | null;
  sortText: string | null;
  filterText: string | null;
  insertText: string;
  insertTextIsSnippet: boolean;
  textEdit: LanguageServerCompletionTextEdit | null;
  additionalTextEdits: LanguageServerCompletionTextEdit[];
  commitCharacters: string[];
  preselect: boolean;
  deprecated: boolean;
  raw: unknown;
};

export type LanguageServerCompletionTextEdit = {
  insert: LanguageServerRange;
  replace: LanguageServerRange;
  newText: string;
};

export type LanguageServerFormattingParams = CloseLanguageServerDocumentParams & {
  languageId: string;
  version: number;
  text: string;
  tabSize: number;
  insertSpaces: boolean;
};

export type LanguageServerTextEdit = {
  range: LanguageServerRange;
  newText: string;
};

export type LanguageServerPrepareRename = {
  serverId: string;
  range: LanguageServerRange | null;
  placeholder: string | null;
};

export type LanguageServerRenameParams = LanguageServerHoverParams & {
  newName: string;
  serverId: string | null;
};

export type LanguageServerCodeActionsParams = LanguageServerDiagnosticsParams & {
  range: LanguageServerRange;
  context: unknown;
};

export type LanguageServerCodeAction = {
  actionId: number;
  serverId: string;
  title: string;
  kind: string | null;
  isPreferred: boolean;
  disabledReason: string | null;
  resolveSupported: boolean;
  commandAuthorization: string | null;
  raw: unknown;
};

export type ResolveLanguageServerCodeActionParams =
  LanguageServerDiagnosticsParams & {
    serverId: string;
    actionId: number;
    raw: unknown;
  };

export type ExecuteLanguageServerCommandParams = LanguageServerDiagnosticsParams & {
  serverId: string;
  authorization: string;
};

export type StagedWorkspaceEdit = {
  transactionId: number;
  authorization: string;
  documents: StagedWorkspaceEditDocument[];
  operations: StagedWorkspaceEditOperation[];
};

export type StagedWorkspaceEditOperation =
  | { kind: "textDocument"; document: number }
  | { kind: "createFile"; workspaceId: WorkspaceId; path: string }
  | {
      kind: "renameFile";
      workspaceId: WorkspaceId;
      oldPath: string;
      newPath: string;
    }
  | { kind: "deleteFile"; workspaceId: WorkspaceId; path: string; recursive: boolean };

export type StagedWorkspaceEditDocument = {
  workspaceId: WorkspaceId;
  path: string;
  originalPath: string;
  originalText: string;
  newText: string;
  generation: number | null;
  version: number | null;
};

export type WorkspaceEditTransactionParams = {
  transactionId: number;
  authorization: string;
};

export type WorkspaceEditTransactionPhase =
  | "staged"
  | "committed"
  | "finishingCommitted"
  | "committedCleanupRequired"
  | "rolledBack"
  | "recoveryRequired"
  | "finishedCommitted"
  | "finishedRolledBack"
  | "finishedUncommitted";

export type WorkspaceEditTransactionStatus = {
  transactionId: number;
  phase: WorkspaceEditTransactionPhase;
  retryRollback: boolean;
  canFinalize: boolean;
  requiresAcknowledgement?: boolean;
};

export type WorkspaceEditRecovery = WorkspaceEditTransactionStatus & {
  authorization: string;
};

export type LanguageServerColor = {
  red: number;
  green: number;
  blue: number;
  alpha: number;
};

export type LanguageServerColorInformation = {
  serverId: string;
  range: LanguageServerRange;
  color: LanguageServerColor;
};

export type LanguageServerColorPresentationParams =
  CloseLanguageServerDocumentParams & {
    version: number;
    serverId: string;
    range: LanguageServerRange;
    color: LanguageServerColor;
  };

export type LanguageServerColorPresentation = {
  label: string;
  textEdit: LanguageServerCompletionTextEdit | null;
  additionalTextEdits: LanguageServerCompletionTextEdit[];
};


export type TrustLanguageServerWorkspaceParams = {
  workspaceId: WorkspaceId;
};
import type { TabId, WorkspaceId } from "./ids";
