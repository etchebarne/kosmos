import type {
  LanguageServerListSnapshot,
  LanguageServerParams,
  LanguageServerSnapshot,
  ChangeLanguageServerDocumentParams,
  CloseLanguageServerDocumentParams,
  LanguageServerHover,
  LanguageServerHoverParams,
  LanguageServerDiagnostic,
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
} from "@/shared/ipc";

import { requestServer } from "./transport";

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
): Promise<LanguageServerHover | null> {
  return requestServer(DOMAIN, "hover", params);
}

export function getLanguageServerDiagnostics(
  params: LanguageServerDiagnosticsParams,
): Promise<LanguageServerDiagnostic[] | null> {
  return requestServer(DOMAIN, "diagnostics", params);
}

export function getLanguageServerCompletions(
  params: LanguageServerCompletionParams,
): Promise<LanguageServerCompletionList> {
  return requestServer(DOMAIN, "completion", params);
}

export function resolveLanguageServerCompletion(
  params: ResolveLanguageServerCompletionParams,
): Promise<LanguageServerCompletionItem> {
  return requestServer(DOMAIN, "resolveCompletion", params);
}

export function requestLanguageServerFormatting(
  params: LanguageServerFormattingParams,
): Promise<LanguageServerTextEdit[]> {
  return requestServer(DOMAIN, "formatting", params);
}

export function getLanguageServerDocumentColors(
  params: LanguageServerDiagnosticsParams,
): Promise<LanguageServerColorInformation[]> {
  return requestServer(DOMAIN, "documentColors", params);
}

export function getLanguageServerColorPresentations(
  params: LanguageServerColorPresentationParams,
): Promise<LanguageServerColorPresentation[]> {
  return requestServer(DOMAIN, "colorPresentations", params);
}


export function trustLanguageServerWorkspace(
  params: TrustLanguageServerWorkspaceParams,
): Promise<boolean> {
  return requestServer(DOMAIN, "trustWorkspace", params);
}
