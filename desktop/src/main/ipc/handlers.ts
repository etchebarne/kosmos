import { BrowserWindow, dialog, ipcMain, shell } from "electron";

import type { KosmosIpcDomain, KosmosIpcError, KosmosIpcRequest, KosmosIpcRequestResult } from "../../shared/ipc";
import { errorMessage } from "../error-message";
import { KosmosIpcRequestError, type KosmosServerClient } from "../server/client";

const validDomains = new Set<KosmosIpcDomain>([
  "workspace",
  "pane",
  "tab",
  "fileTree",
  "git",
  "terminal",
]);

export function registerIpcHandlers(serverClient: KosmosServerClient): void {
  ipcMain.handle("kosmos:request", async (_event, request: KosmosIpcRequest): Promise<KosmosIpcRequestResult> => {
    try {
      validateRequest(request);
      const result = await serverClient.request(request.domain, request.action, request.params ?? {});

      return { ok: true, result };
    } catch (caughtError: unknown) {
      return { ok: false, error: ipcRequestError(caughtError) };
    }
  });

  ipcMain.handle("kosmos:selectWorkspaceDirectory", async () => {
    const result = await dialog.showOpenDialog({
      properties: ["openDirectory"],
      title: "Open Workspace",
    });

    return result.canceled ? undefined : result.filePaths[0];
  });

  ipcMain.handle("kosmos:window:minimize", (event) => {
    BrowserWindow.fromWebContents(event.sender)?.minimize();
  });

  ipcMain.handle("kosmos:window:toggleMaximize", (event) => {
    const window = BrowserWindow.fromWebContents(event.sender);

    if (!window) {
      return;
    }

    if (window.isMaximized()) {
      window.unmaximize();
    } else {
      window.maximize();
    }
  });

  ipcMain.handle("kosmos:window:close", (event) => {
    BrowserWindow.fromWebContents(event.sender)?.close();
  });

  ipcMain.handle("kosmos:revealPath", (_event, targetPath: unknown) => {
    if (typeof targetPath !== "string" || targetPath.length === 0) {
      throw new Error("Reveal path must be a non-empty string");
    }

    shell.showItemInFolder(targetPath);
  });
}

function validateRequest(request: KosmosIpcRequest): void {
  if (!request || typeof request !== "object") {
    throw new Error("IPC request must be an object");
  }

  if (!validDomains.has(request.domain)) {
    throw new Error(`Unsupported IPC domain: ${String(request.domain)}`);
  }

  if (typeof request.action !== "string" || request.action.length === 0) {
    throw new Error("IPC request action must be a non-empty string");
  }
}

function ipcRequestError(error: unknown): KosmosIpcError {
  if (error instanceof KosmosIpcRequestError) {
    return { code: error.code, message: error.messageWithoutCode };
  }

  return { code: "ipc.request_failed", message: errorMessage(error) };
}
