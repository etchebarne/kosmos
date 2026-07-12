import type * as Generated from "./generated/types";

export type * from "./generated/types";

export const APPEARANCE_ZOOM_LEVEL = "appearance.zoomLevel";

export type WorkspaceId = Generated.WorkspaceIdParam;
export type PaneId = Generated.PaneIdParam;
export type SplitPaneId = Generated.SplitPaneIdParam;
export type TabId = Generated.TabIdParam;

export type SplitAxis = Generated.SplitAxisPayload;
export type TabKind = Generated.TabKindPayload;
export type OpenableTabKind = Exclude<TabKind, "diff" | "editor">;
export type TabLifecycle = Generated.TabLifecyclePayload;
export type FileTreeEntryKind = Generated.FileTreeEntryKindParam;
export type GitChangeKind = Generated.GitChangeKindPayload;
export type GitDiffSectionKind = Generated.GitDiffSectionKindPayload;
export type SearchMode = Generated.SearchModeParam;

export type EditorTabParams = Pick<Generated.EditorDocumentParams, "workspaceId" | "tabId">;
export type EditorDocument = Generated.EditorDocumentPayload;
export type EditorGitLineHunks = Generated.EditorGitLineHunksPayload;
export type EditorGitLineHunk = Generated.EditorGitLineHunkPayload;
export type CloseResult = Generated.CloseResultPayload;
export type UnsavedDocument = Generated.UnsavedDocumentPayload;
export type CloseDocumentDecision = Generated.CloseDocumentDecisionKindPayload;

export type GitBranch = Generated.GitBranchPayload;
export type GitChange = Generated.GitChangePayload;
export type GitRepositorySnapshot = Generated.GitRepositorySnapshotPayload;
export type GitStash = Generated.GitStashPayload;
export type GitRemote = Generated.GitRemotePayload;
export type GitTag = Generated.GitTagPayload;
export type GitDiffSection = Generated.GitDiffSectionPayload;
export type GitDiffFile = Generated.GitDiffFilePayload;
export type GitDiff = Generated.GitDiffPayload;

export type FormatterInstallationState = Generated.InstallationStatePayload;
export type FormatterFailure = Generated.FormatterFailurePayload;

export type SearchMatch = Generated.SearchMatchPayload;
export type WorkspaceSearchResults = Generated.WorkspaceSearchResultsPayload;
export type SearchDocument = EditorDocument;

export type TerminalShell = Generated.TerminalShellSnapshot;
export type TerminalOutput = Generated.TerminalOutputSnapshot;

export type SettingValue = Generated.SettingValuePayload;
export type SettingControl = Generated.SettingControlPayload;
export type SettingOption = Generated.SettingOptionPayload;
export type SettingItem = Generated.SettingItemPayload;
export type SettingGroup = Extract<SettingItem, { type: "group" }>;
export type SettingDefinition = Extract<SettingItem, { type: "setting" }>;
export type SettingCategory = Generated.SettingCategoryPayload;

export type WindowState = Generated.WindowStateSnapshot;

export type LanguageServerInstallationState = Generated.InstallationStatePayload;
export type LanguageServerRuntimeState = Generated.RuntimeStatePayload;
export type LanguageServerFailure = Generated.LanguageServerFailurePayload;
export type LanguageServerLog = Generated.LanguageServerLogPayload;
export type LanguageServerPosition = Generated.LanguageServerPositionPayload;
export type LanguageServerRange = Generated.LanguageServerRangePayload;
export type LanguageServerChange = Generated.LanguageServerChangePayload;
export type LanguageServerHover = Generated.LanguageServerHoverPayload;
export type LanguageServerSignatureHelp = Generated.LanguageServerSignatureHelpPayload;
export type LanguageServerSignatureInformation = Generated.LanguageServerSignatureInformationPayload;
export type LanguageServerLocation = Generated.LanguageServerLocationPayload;
export type LanguageServerDocumentSymbol = Generated.LanguageServerDocumentSymbolPayload;
export type LanguageServerWorkspaceSymbol = Generated.LanguageServerWorkspaceSymbolPayload;
export type LanguageServerDiagnostic = Generated.LanguageServerDiagnosticPayload;
export type LanguageServerDiagnosticSnapshot = Generated.LanguageServerDiagnosticSnapshotPayload;
export type LanguageServerDiagnosticsChanged = Generated.LanguageServerDiagnosticsChangedNotification;
export type LanguageServerCompletionList = Generated.LanguageServerCompletionListPayload;
export type LanguageServerCompletionItem = Generated.LanguageServerCompletionItemPayload;
export type LanguageServerCompletionTextEdit = Generated.LanguageServerCompletionTextEditPayload;
export type LanguageServerTextEdit = Generated.LanguageServerTextEditPayload;
export type LanguageServerPrepareRename = Generated.LanguageServerPrepareRenamePayload;
export type LanguageServerCodeAction = Generated.LanguageServerCodeActionPayload;
export type WorkspaceEditModelDirective = {
  workspaceId: WorkspaceId;
  originalPath: string;
  path: string | null;
  generation: number;
  version: number;
  originalText: string;
  text: string;
};
export type WorkspaceEditDirective =
  | {
    kind: "applyOpenModels" | "undoOpenModels";
    transactionId: number;
    models: WorkspaceEditModelDirective[];
  }
  | {
    kind: "reconcileCommittedModels" | "reconcileRolledBackModels";
    transactionId: number;
  };
export type StagedWorkspaceEdit = Generated.StagedWorkspaceEditPayload & {
  directive?: WorkspaceEditDirective;
};
export type StagedWorkspaceEditDocument = Generated.StagedWorkspaceEditDocumentPayload;
export type StagedWorkspaceEditOperation = Generated.StagedWorkspaceEditOperationPayload;
export type WorkspaceEditTransactionStatus = Generated.WorkspaceEditTransactionStatusPayload;
export type WorkspaceEditRecovery = Generated.WorkspaceEditRecoveryPayload;
export type LanguageServerColor = Generated.LanguageServerColorPayload;
export type LanguageServerColorInformation = Generated.LanguageServerColorInformationPayload;
export type LanguageServerColorPresentation = Generated.LanguageServerColorPresentationPayload;

export type KosmosIpcParams = object;
export type KosmosIpcRequest = {
  domain: Generated.KosmosIpcDomain;
  action: string;
  params?: KosmosIpcParams;
  requestKey?: string;
};
export type KosmosServerResponse =
  | { type: "response"; id: number; ok: true; result: unknown }
  | { type: "response"; id: number; ok: false; error: Generated.KosmosIpcError };
export type KosmosServerNotification =
  | Generated.WorkspaceChangedNotification
  | Generated.LanguageServerDiagnosticsChangedNotification
  | Generated.LanguageServerDiagnosticsResyncNotification
  | Generated.LanguageServerStatusChangedNotification
  | Generated.LanguageServerLogAvailableNotification
  | (Generated.LanguageServerApplyEditNotification & { edit: StagedWorkspaceEdit })
  | Generated.LanguageServerApplyEditCancelledNotification;
export type KosmosServerMessage = KosmosServerResponse | KosmosServerNotification;
export type KosmosIpcRequestResult<T = unknown> =
  | { ok: true; result: T }
  | { ok: false; error: Generated.KosmosIpcError };

export type KosmosApi = {
  request<T = unknown>(request: KosmosIpcRequest): Promise<KosmosIpcRequestResult<T>>;
  cancelRequest(requestKey: string): void;
  acknowledgeServerApplyEdit(
    id: number,
    token: string,
    applied: boolean,
    failureReason?: string,
  ): void;
  pendingServerApplyEdits(): Promise<
    Array<Extract<KosmosServerNotification, { event: "languageServerApplyEdit" }>>
  >;
  selectWorkspaceDirectory(): Promise<string | undefined>;
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  setZoomLevel(zoomLevel: number): Promise<void>;
  revealPath(path: string): Promise<void>;
  onFlushState(callback: () => Promise<void>): () => void;
  onShutdownRequest(callback: () => Promise<boolean>): () => void;
  onZoomLevelChanged(callback: (zoomLevel: number) => void): () => void;
  onWorkspaceChanged(callback: (workspaceIds: WorkspaceId[]) => void): () => void;
  onServerNotification(callback: (notification: KosmosServerNotification) => void): () => void;
  onServerReconnected(callback: () => void): () => void;
};
