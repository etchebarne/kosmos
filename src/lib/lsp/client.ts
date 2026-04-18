import {
  type InitializeParams,
  type InitializeResult,
  type ServerCapabilities,
  type TextDocumentContentChangeEvent,
  type CompletionItem,
  type CompletionList,
  type Hover,
  type Location,
  type LocationLink,
  type SignatureHelp,
  type PublishDiagnosticsParams,
  type DidOpenTextDocumentParams,
  type DidChangeTextDocumentParams,
  type DidCloseTextDocumentParams,
  type DidSaveTextDocumentParams,
  type CompletionParams,
  type HoverParams,
  type TextDocumentPositionParams,
  type ReferenceParams,
  type SignatureHelpParams,
  type CodeAction,
  type CodeActionParams,
  type Command,
  type DocumentFormattingParams,
  type DocumentRangeFormattingParams,
  type RenameParams,
  type PrepareRenameParams,
  type TextEdit,
  type WorkspaceEdit,
  type DocumentSymbol,
  type SymbolInformation,
  type DocumentSymbolParams,
  type Range,
  TextDocumentSyncKind,
  type FileEvent,
  type ConfigurationParams,
} from "vscode-languageserver-protocol";
import { TauriLspTransport } from "./transport";
import { pathToFileUri } from "./uri";

// Work-done progress (LSP 3.15+).

interface WorkDoneProgressBegin {
  kind: "begin";
  title: string;
  cancellable?: boolean;
  message?: string;
  percentage?: number;
}

interface WorkDoneProgressReport {
  kind: "report";
  cancellable?: boolean;
  message?: string;
  percentage?: number;
}

interface WorkDoneProgressEnd {
  kind: "end";
  message?: string;
}

type WorkDoneProgressValue = WorkDoneProgressBegin | WorkDoneProgressReport | WorkDoneProgressEnd;

interface ProgressParams {
  token: string | number;
  value: WorkDoneProgressValue;
}

export class LspClient {
  private transport: TauriLspTransport;
  capabilities: ServerCapabilities | null = null;
  private openDocuments = new Set<string>();
  /** WSL prefix variants: raw "wsl://Ubuntu" and encoded "wsl%3A//Ubuntu". */
  private wslRaw: string | null = null;
  private wslEncoded: string | null = null;

  constructor(transport: TauriLspTransport, wslPrefix?: string) {
    this.transport = transport;
    this.wslRaw = wslPrefix ?? null;
    this.wslEncoded = wslPrefix ? wslPrefix.replace(":", "%3A") : null;

    // Stub responses for server→client requests we don't implement;
    // silence is treated as an error by many servers.
    this.transport.onRequest("window/workDoneProgress/create", () => null);
    this.transport.onRequest("client/registerCapability", () => null);
    this.transport.onRequest("client/unregisterCapability", () => null);
    this.transport.onRequest("workspace/configuration", (params) => {
      const p = params as ConfigurationParams | null;
      return (p?.items ?? []).map(() => ({}));
    });
    this.transport.onRequest("workspace/workspaceFolders", () => null);
    this.transport.onRequest("window/showMessageRequest", () => null);
    this.transport.onRequest("workspace/codeLens/refresh", () => null);
    this.transport.onRequest("workspace/semanticTokens/refresh", () => null);
    this.transport.onRequest("workspace/inlayHint/refresh", () => null);
    this.transport.onRequest("workspace/diagnostics/refresh", () => null);
  }

  /** Strip the wsl:// prefix so the server sees native Linux paths. */
  toServerUri(uri: string): string {
    if (!this.wslEncoded) return uri;
    return uri.replace(`/${this.wslEncoded}`, "").replace(`/${this.wslRaw}`, "");
  }

  /** Re-inject the wsl:// prefix so the editor resolves to the remote workspace. */
  fromServerUri(uri: string): string {
    if (!this.wslEncoded) return uri;
    return uri.replace("file:///", `file:///${this.wslEncoded}/`);
  }

  /** Subscribe to work-done progress updates (indexing, loading, etc.). */
  onProgress(handler: (token: string | number, value: WorkDoneProgressValue) => void): void {
    this.transport.onNotification("$/progress", (params: unknown) => {
      const p = params as ProgressParams;
      if (p?.token != null && p?.value?.kind) {
        handler(p.token, p.value);
      }
    });
  }

  async initialize(workspacePath: string): Promise<InitializeResult> {
    const rootUri = this.toServerUri(pathToFileUri(workspacePath));
    const noDynamic = { dynamicRegistration: false } as const;

    const params: InitializeParams = {
      processId: null,
      rootUri,
      capabilities: {
        textDocument: {
          synchronization: {
            ...noDynamic,
            willSave: false,
            willSaveWaitUntil: false,
            didSave: true,
          },
          completion: {
            ...noDynamic,
            completionItem: {
              snippetSupport: true,
              commitCharactersSupport: true,
              documentationFormat: ["markdown", "plaintext"],
              deprecatedSupport: true,
              preselectSupport: true,
              labelDetailsSupport: true,
              insertReplaceSupport: true,
              resolveSupport: {
                properties: ["documentation", "detail", "additionalTextEdits"],
              },
            },
            contextSupport: true,
          },
          hover: {
            ...noDynamic,
            contentFormat: ["markdown", "plaintext"],
          },
          signatureHelp: {
            ...noDynamic,
            signatureInformation: {
              documentationFormat: ["markdown", "plaintext"],
              parameterInformation: { labelOffsetSupport: true },
            },
          },
          definition: noDynamic,
          references: noDynamic,
          documentHighlight: noDynamic,
          documentSymbol: {
            ...noDynamic,
            hierarchicalDocumentSymbolSupport: true,
          },
          codeAction: {
            ...noDynamic,
            codeActionLiteralSupport: {
              codeActionKind: {
                valueSet: [
                  "quickfix",
                  "refactor",
                  "refactor.extract",
                  "refactor.inline",
                  "refactor.rewrite",
                  "source",
                  "source.organizeImports",
                ],
              },
            },
            resolveSupport: {
              properties: ["edit"],
            },
          },
          formatting: noDynamic,
          rangeFormatting: noDynamic,
          rename: { ...noDynamic, prepareSupport: true },
          publishDiagnostics: {
            relatedInformation: true,
            tagSupport: { valueSet: [1, 2] },
          },
        },
        workspace: {
          workspaceFolders: true,
          didChangeWatchedFiles: noDynamic,
        },
        window: {
          workDoneProgress: true,
        },
      },
      workspaceFolders: [
        {
          uri: rootUri,
          name:
            workspacePath
              .split(/[\\/]/)
              .pop()
              ?.replace(/^wsl:\/\/[^/]+/, "") ?? "",
        },
      ],
    };

    const result = await this.transport.sendRequest<InitializeResult>("initialize", params);
    this.capabilities = result.capabilities;

    this.transport.sendNotification("initialized", {});

    return result;
  }

  didOpen(uri: string, languageId: string, version: number, text: string): void {
    if (this.openDocuments.has(uri)) return;
    this.openDocuments.add(uri);

    const serverUri = this.toServerUri(uri);
    const params: DidOpenTextDocumentParams = {
      textDocument: { uri: serverUri, languageId, version, text },
    };
    this.transport.sendNotification("textDocument/didOpen", params);
  }

  didChange(uri: string, version: number, changes: TextDocumentContentChangeEvent[]): void {
    const syncKind =
      typeof this.capabilities?.textDocumentSync === "object"
        ? this.capabilities.textDocumentSync.change
        : this.capabilities?.textDocumentSync;

    // Full-sync servers get the full text in every change; drop intermediates.
    const actualChanges =
      syncKind === TextDocumentSyncKind.Full && changes.length > 1
        ? [changes[changes.length - 1]]
        : changes;

    const params: DidChangeTextDocumentParams = {
      textDocument: { uri: this.toServerUri(uri), version },
      contentChanges: actualChanges,
    };
    this.transport.sendNotification("textDocument/didChange", params);
  }

  didClose(uri: string): void {
    if (!this.openDocuments.has(uri)) return;
    this.openDocuments.delete(uri);

    const params: DidCloseTextDocumentParams = {
      textDocument: { uri: this.toServerUri(uri) },
    };
    this.transport.sendNotification("textDocument/didClose", params);
  }

  didSave(uri: string, text?: string): void {
    const params: DidSaveTextDocumentParams = {
      textDocument: { uri: this.toServerUri(uri) },
      ...(text !== undefined && { text }),
    };
    this.transport.sendNotification("textDocument/didSave", params);
  }

  /** Clear open document tracking (call before reusing client after server restart). */
  clearOpenDocuments(): void {
    this.openDocuments.clear();
  }

  async completion(
    uri: string,
    line: number,
    character: number,
  ): Promise<CompletionList | CompletionItem[] | null> {
    const params: CompletionParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
    };
    return this.transport.sendRequest("textDocument/completion", params);
  }

  async completionResolve(item: CompletionItem): Promise<CompletionItem> {
    return this.transport.sendRequest("completionItem/resolve", item);
  }

  async hover(uri: string, line: number, character: number): Promise<Hover | null> {
    const params: HoverParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
    };
    return this.transport.sendRequest("textDocument/hover", params);
  }

  async definition(
    uri: string,
    line: number,
    character: number,
  ): Promise<Location | Location[] | LocationLink[] | null> {
    const params: TextDocumentPositionParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
    };
    return this.transport.sendRequest("textDocument/definition", params);
  }

  async references(uri: string, line: number, character: number): Promise<Location[] | null> {
    const params: ReferenceParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
      context: { includeDeclaration: true },
    };
    return this.transport.sendRequest("textDocument/references", params);
  }

  async signatureHelp(uri: string, line: number, character: number): Promise<SignatureHelp | null> {
    const params: SignatureHelpParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
    };
    return this.transport.sendRequest("textDocument/signatureHelp", params);
  }

  async codeAction(
    uri: string,
    range: Range,
    diagnostics: { range: Range; message: string; severity?: number; code?: number | string }[],
  ): Promise<(CodeAction | Command)[] | null> {
    const params: CodeActionParams = {
      textDocument: { uri: this.toServerUri(uri) },
      range,
      context: { diagnostics: diagnostics as CodeActionParams["context"]["diagnostics"] },
    };
    return this.transport.sendRequest("textDocument/codeAction", params);
  }

  async formatting(
    uri: string,
    tabSize: number,
    insertSpaces: boolean,
  ): Promise<TextEdit[] | null> {
    const params: DocumentFormattingParams = {
      textDocument: { uri: this.toServerUri(uri) },
      options: { tabSize, insertSpaces },
    };
    return this.transport.sendRequest("textDocument/formatting", params);
  }

  async rangeFormatting(
    uri: string,
    range: Range,
    tabSize: number,
    insertSpaces: boolean,
  ): Promise<TextEdit[] | null> {
    const params: DocumentRangeFormattingParams = {
      textDocument: { uri: this.toServerUri(uri) },
      range,
      options: { tabSize, insertSpaces },
    };
    return this.transport.sendRequest("textDocument/rangeFormatting", params);
  }

  async prepareRename(
    uri: string,
    line: number,
    character: number,
  ): Promise<Range | { range: Range; placeholder: string } | null> {
    const params: PrepareRenameParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
    };
    return this.transport.sendRequest("textDocument/prepareRename", params);
  }

  async rename(
    uri: string,
    line: number,
    character: number,
    newName: string,
  ): Promise<WorkspaceEdit | null> {
    const params: RenameParams = {
      textDocument: { uri: this.toServerUri(uri) },
      position: { line, character },
      newName,
    };
    return this.transport.sendRequest("textDocument/rename", params);
  }

  async documentSymbol(uri: string): Promise<DocumentSymbol[] | SymbolInformation[] | null> {
    const params: DocumentSymbolParams = {
      textDocument: { uri: this.toServerUri(uri) },
    };
    return this.transport.sendRequest("textDocument/documentSymbol", params);
  }

  didChangeWatchedFiles(changes: FileEvent[]): void {
    this.transport.sendNotification("workspace/didChangeWatchedFiles", { changes });
  }

  onDiagnostics(handler: (params: PublishDiagnosticsParams) => void): void {
    this.transport.onNotification("textDocument/publishDiagnostics", ((params: unknown) => {
      const p = params as PublishDiagnosticsParams;
      if (p?.uri) p.uri = this.fromServerUri(p.uri);
      handler(p);
    }) as (params: unknown) => void);
  }

  async shutdown(): Promise<void> {
    await this.transport.sendRequest("shutdown", null);
    this.transport.sendNotification("exit", null);
    this.transport.dispose();
  }

  dispose(): void {
    this.openDocuments.clear();
    this.transport.dispose();
  }
}
