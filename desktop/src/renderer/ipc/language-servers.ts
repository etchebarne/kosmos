import type {
  LanguageServerListSnapshot,
  LanguageServerParams,
  LanguageServerSnapshot,
  ChangeLanguageServerDocumentParams,
  CloseLanguageServerDocumentParams,
  LanguageServerHover,
  LanguageServerHoverParams,
  LanguageServerDiagnosticSnapshot,
  LanguageServerDiagnosticsParams,
  LanguageServerCompletionItem,
  LanguageServerCompletionList,
  LanguageServerCompletionParams,
  LanguageServerFormattingParams,
  LanguageServerColorInformation,
  LanguageServerColorPresentation,
  LanguageServerColorPresentationParams,
  OpenLanguageServerDocumentParams,
  TrustLanguageServerWorkspaceParams,
  ResolveLanguageServerCompletionParams,
  SaveLanguageServerDocumentParams,
  LanguageServerTextEdit,
  LanguageServerSignatureHelp,
  LanguageServerLocation,
  LanguageServerReferencesParams,
  LanguageServerDocumentSymbol,
  LanguageServerWorkspaceSymbol,
  LanguageServerWorkspaceSymbolsParams,
  ResolveLanguageServerWorkspaceSymbolParams,
  ExecuteLanguageServerCommandParams,
  LanguageServerCodeAction,
  LanguageServerCodeActionsParams,
  LanguageServerPrepareRename,
  LanguageServerRenameParams,
  ResolveLanguageServerCodeActionParams,
  StagedWorkspaceEdit,
  WorkspaceEditTransactionParams,
  WorkspaceEditTransactionStatus,
  WorkspaceEditRecovery,
} from "@/shared/ipc";

import { requestServer, type RequestCancellation } from "./transport";

const DOMAIN = "languageServers";

export function listLanguageServers(): Promise<LanguageServerListSnapshot> {
  return requestServer(DOMAIN, "list");
}

export function getLanguageServerStatus(
  params: LanguageServerParams,
): Promise<LanguageServerSnapshot> {
  return requestServer(DOMAIN, "status", params);
}

export function installLanguageServer(
  params: LanguageServerParams,
): Promise<LanguageServerSnapshot> {
  return requestServer(DOMAIN, "install", params);
}

export function uninstallLanguageServer(
  params: LanguageServerParams,
): Promise<LanguageServerSnapshot> {
  return requestServer(DOMAIN, "uninstall", params);
}

export function restartLanguageServer(
  params: LanguageServerParams,
): Promise<LanguageServerSnapshot> {
  return requestServer(DOMAIN, "restart", params);
}

export function openLanguageServerDocument(
  params: OpenLanguageServerDocumentParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "openDocument", params);
}

export function changeLanguageServerDocument(
  params: ChangeLanguageServerDocumentParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "changeDocument", params);
}

export function closeLanguageServerDocument(
  params: CloseLanguageServerDocumentParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "closeDocument", params);
}

export function saveLanguageServerDocument(
  params: SaveLanguageServerDocumentParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "saveDocument", params);
}

export function getLanguageServerHover(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerHover | null> {
  return requestLanguageServerFeature("hover", params, cancellation);
}

export function getLanguageServerSignatureHelp(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerSignatureHelp | null> {
  return requestLanguageServerFeature("signatureHelp", params, cancellation);
}

export function getLanguageServerDefinitions(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerLocation[]> {
  return requestLanguageServerFeature("definition", params, cancellation);
}

export function getLanguageServerDeclarations(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerLocation[]> {
  return requestLanguageServerFeature("declaration", params, cancellation);
}

export function getLanguageServerTypeDefinitions(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerLocation[]> {
  return requestLanguageServerFeature("typeDefinition", params, cancellation);
}

export function getLanguageServerImplementations(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerLocation[]> {
  return requestLanguageServerFeature("implementation", params, cancellation);
}

export function getLanguageServerReferences(
  params: LanguageServerReferencesParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerLocation[]> {
  return requestLanguageServerFeature("references", params, cancellation);
}

export function getLanguageServerDocumentSymbols(
  params: LanguageServerDiagnosticsParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerDocumentSymbol[]> {
  return requestLanguageServerFeature("documentSymbols", params, cancellation);
}

export function getLanguageServerWorkspaceSymbols(
  params: LanguageServerWorkspaceSymbolsParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerWorkspaceSymbol[]> {
  return requestLanguageServerFeature("workspaceSymbols", params, cancellation);
}

export function resolveLanguageServerWorkspaceSymbol(
  params: ResolveLanguageServerWorkspaceSymbolParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerWorkspaceSymbol> {
  return requestLanguageServerFeature("resolveWorkspaceSymbol", params, cancellation);
}

export function getLanguageServerDiagnostics(
  params: LanguageServerDiagnosticsParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerDiagnosticSnapshot[] | null> {
  return requestLanguageServerFeature("diagnostics", params, cancellation);
}

export function getLanguageServerCompletions(
  params: LanguageServerCompletionParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerCompletionList> {
  return requestLanguageServerFeature("completion", params, cancellation);
}

export function resolveLanguageServerCompletion(
  params: ResolveLanguageServerCompletionParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerCompletionItem> {
  return requestLanguageServerFeature("resolveCompletion", params, cancellation);
}

export function requestLanguageServerFormatting(
  params: LanguageServerFormattingParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerTextEdit[]> {
  return requestLanguageServerFeature("formatting", params, cancellation);
}

export function prepareLanguageServerRename(
  params: LanguageServerHoverParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerPrepareRename | null> {
  return requestLanguageServerFeature("prepareRename", params, cancellation);
}

export function requestLanguageServerRename(
  params: LanguageServerRenameParams,
  cancellation?: RequestCancellation,
): Promise<StagedWorkspaceEdit> {
  return requestLanguageServerFeature("rename", params, cancellation);
}

export function getLanguageServerCodeActions(
  params: LanguageServerCodeActionsParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerCodeAction[]> {
  return requestLanguageServerFeature("codeActions", params, cancellation);
}

export function resolveLanguageServerCodeAction(
  params: ResolveLanguageServerCodeActionParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerCodeAction> {
  return requestLanguageServerFeature("resolveCodeAction", params, cancellation);
}

export function stageLanguageServerCodeAction(
  action: LanguageServerCodeAction,
): Promise<StagedWorkspaceEdit | null> {
  return requestServer(DOMAIN, "stageCodeAction", { action });
}

export function executeLanguageServerCommand(
  params: ExecuteLanguageServerCommandParams,
  cancellation?: RequestCancellation,
): Promise<unknown> {
  return requestLanguageServerFeature("executeCommand", params, cancellation);
}

export function commitWorkspaceEdit(params: WorkspaceEditTransactionParams): Promise<boolean> {
  return requestServer(DOMAIN, "commitWorkspaceEdit", params);
}

export function rollbackWorkspaceEdit(params: WorkspaceEditTransactionParams): Promise<boolean> {
  return requestServer(DOMAIN, "rollbackWorkspaceEdit", params);
}

export function finishWorkspaceEdit(params: WorkspaceEditTransactionParams): Promise<boolean> {
  return requestServer(DOMAIN, "finishWorkspaceEdit", params);
}

export function acknowledgeWorkspaceEditCompletion(
  params: WorkspaceEditTransactionParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "acknowledgeWorkspaceEditCompletion", params);
}

export function finalizeWorkspaceEdit(
  params: WorkspaceEditTransactionParams,
): Promise<WorkspaceEditTransactionStatus> {
  return requestServer(DOMAIN, "finalizeWorkspaceEdit", params);
}

export function getWorkspaceEditStatus(
  params: WorkspaceEditTransactionParams,
): Promise<WorkspaceEditTransactionStatus> {
  return requestServer(DOMAIN, "workspaceEditStatus", params);
}

export function listWorkspaceEditRecoveries(): Promise<WorkspaceEditRecovery[]> {
  return requestServer(DOMAIN, "listWorkspaceEditRecoveries");
}

export function getLanguageServerDocumentColors(
  params: LanguageServerDiagnosticsParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerColorInformation[]> {
  return requestLanguageServerFeature("documentColors", params, cancellation);
}

export function getLanguageServerColorPresentations(
  params: LanguageServerColorPresentationParams,
  cancellation?: RequestCancellation,
): Promise<LanguageServerColorPresentation[]> {
  return requestLanguageServerFeature("colorPresentations", params, cancellation);
}

export function requestLanguageServerFeature<T>(
  action: string,
  params: Record<string, unknown>,
  cancellation?: RequestCancellation,
): Promise<T> {
  return requestServer(DOMAIN, action, params, cancellation);
}


export function trustLanguageServerWorkspace(
  params: TrustLanguageServerWorkspaceParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "trustWorkspace", params);
}
