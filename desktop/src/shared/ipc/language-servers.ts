export type LanguageServerInstallationState =
  | "notInstalled"
  | "installing"
  | "installed"
  | "uninstalling"
  | "failed";

export type LanguageServerRuntimeState = "inactive" | "running" | "degraded" | "crashed";

export type LanguageServerFailure = {
  code: string;
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

export type LanguageServerDiagnostic = {
  range: LanguageServerRange;
  severity: "error" | "warning" | "information" | "hint" | null;
  message: string;
  source: string | null;
  code: string | null;
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
