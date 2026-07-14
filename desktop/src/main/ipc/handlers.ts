import {
  BrowserWindow,
  dialog,
  ipcMain,
  shell,
  type WebContents,
  type WebFrameMain,
} from "electron";

import type {
  KosmosIpcDomain,
  KosmosIpcRequest,
  KosmosIpcRequestResult,
  KosmosServerNotification,
  SettingsSnapshot,
} from "../../shared/ipc";
import type { KosmosServerClient } from "../server/client";
import { isSettingsSnapshot } from "../settings-snapshot";
import { setWindowZoomLevel, updateWindowZoomPolicy } from "../window/shortcuts";
import { isTrustedRendererUrl } from "../window/security";
import { ipcRequestFailure } from "./request-result";

export type ApplyEditOwner = {
  id: number;
  webContentsId: number;
  notification: Extract<KosmosServerNotification, { event: "languageServerApplyEdit" }>;
  cleanup(): void;
};

export type ServerRecoveryState = {
  readonly active: boolean;
  readonly generation: number;
  readonly restoringRenderer: boolean;
  rendererAvailable(): void;
  rendererComplete(generation: number, error?: string): void;
};

const validDomains = new Set<KosmosIpcDomain>([
  "workspace",
  "pane",
  "tab",
  "fileTree",
  "formatters",
  "editor",
  "git",
  "search",
  "terminal",
  "settings",
  "languageServers",
  "window",
]);

export function registerIpcHandlers(
  serverClient: KosmosServerClient,
  applyEditOwners: Map<string, ApplyEditOwner>,
  settingsSnapshots: Map<number, SettingsSnapshot>,
  rendererEntryPath: string,
  readyRenderers: Set<number>,
  serverRecovery: ServerRecoveryState,
): void {
  const rendererRequests = new Map<number, Map<string, AbortController>>();
  const watchedRenderers = new Set<number>();
  const watchedReadyRenderers = new Set<number>();

  ipcMain.on("kosmos:rendererReady", (event) => {
    if (!isTrustedIpcSender(event, rendererEntryPath)) {
      return;
    }
    readyRenderers.add(event.sender.id);
    serverRecovery.rendererAvailable();
    if (!watchedReadyRenderers.has(event.sender.id)) {
      watchedReadyRenderers.add(event.sender.id);
      const clearReady = () => readyRenderers.delete(event.sender.id);
      event.sender.once("destroyed", () => {
        clearReady();
        watchedReadyRenderers.delete(event.sender.id);
      });
      event.sender.on("render-process-gone", clearReady);
      event.sender.on("did-start-navigation", (_event, _url, _isInPlace, isMainFrame) => {
        if (isMainFrame) {
          clearReady();
        }
      });
    }
  });
  ipcMain.on("kosmos:serverRecoveryComplete", (event, result: unknown) => {
    if (!isTrustedIpcSender(event, rendererEntryPath)) {
      return;
    }
    if (
      !result ||
      typeof result !== "object" ||
      !("generation" in result) ||
      typeof result.generation !== "number" ||
      !Number.isSafeInteger(result.generation) ||
      result.generation !== serverRecovery.generation
    ) {
      return;
    }
    const reportedError = "error" in result ? result.error : undefined;
    if (reportedError !== undefined && typeof reportedError !== "string") {
      return;
    }
    const error =
      typeof reportedError === "string"
        ? reportedError.slice(0, 4_096) || "Renderer recovery failed without an error message."
        : undefined;
    serverRecovery.rendererComplete(result.generation, error);
  });

  ipcMain.on("kosmos:cancelRequest", (event, requestKey: unknown) => {
    if (!isTrustedIpcSender(event, rendererEntryPath)) {
      return;
    }
    if (typeof requestKey !== "string") {
      return;
    }
    rendererRequests.get(event.sender.id)?.get(requestKey)?.abort();
  });
  ipcMain.on("kosmos:serverApplyEditAck", (event, acknowledgement: unknown) => {
    if (!isTrustedIpcSender(event, rendererEntryPath)) {
      return;
    }
    if (
      !acknowledgement ||
      typeof acknowledgement !== "object" ||
      !("id" in acknowledgement) ||
      typeof acknowledgement.id !== "number" ||
      !Number.isSafeInteger(acknowledgement.id) ||
      acknowledgement.id < 0 ||
      !("token" in acknowledgement) ||
      typeof acknowledgement.token !== "string" ||
      !("applied" in acknowledgement) ||
      typeof acknowledgement.applied !== "boolean"
    ) {
      return;
    }
    const owner = applyEditOwners.get(acknowledgement.token);
    if (!owner || owner.id !== acknowledgement.id || owner.webContentsId !== event.sender.id) {
      return;
    }
    applyEditOwners.delete(acknowledgement.token);
    owner.cleanup();
    const failureReason =
      "failureReason" in acknowledgement && typeof acknowledgement.failureReason === "string"
        ? acknowledgement.failureReason.slice(0, 4_096)
        : undefined;
    void serverClient.acknowledgeApplyEdit(
      acknowledgement.id,
      acknowledgement.token,
      acknowledgement.applied,
      failureReason,
    );
  });

  ipcMain.handle("kosmos:request", async (event, request: KosmosIpcRequest): Promise<KosmosIpcRequestResult> => {
    let cancellation: AbortController | undefined;
    try {
      assertTrustedIpcSender(event, rendererEntryPath);
      validateRequest(request);
      assertRequestAllowedDuringRecovery(request, serverRecovery);
      cancellation = beginRendererRequest(
        rendererRequests,
        watchedRenderers,
        event.sender,
        request.requestKey,
      );
      const result = await serverClient.request(
        request.domain,
        request.action,
        request.params ?? {},
        cancellation?.signal,
      );

      updateWindowSettingsPolicy(event.sender, request, result, settingsSnapshots);

      return { ok: true, result };
    } catch (caughtError: unknown) {
      return ipcRequestFailure(caughtError);
    } finally {
      if (cancellation && request.requestKey) {
        const requests = rendererRequests.get(event.sender.id);
        if (requests?.get(request.requestKey) === cancellation) {
          requests.delete(request.requestKey);
        }
      }
    }
  });
  ipcMain.handle("kosmos:pendingServerApplyEdits", (event) => {
    assertTrustedIpcSender(event, rendererEntryPath);
    return [...applyEditOwners.values()]
      .filter((owner) => owner.webContentsId === event.sender.id)
      .map((owner) => owner.notification);
  });
  ipcMain.handle("kosmos:bootstrapSettings", (event) => {
    assertTrustedIpcSender(event, rendererEntryPath);
    const snapshot = settingsSnapshots.get(event.sender.id);
    if (!snapshot) {
      throw new Error("Bootstrap settings are unavailable for this window");
    }

    return snapshot;
  });

  ipcMain.handle("kosmos:selectWorkspaceDirectory", async (event) => {
    assertTrustedIpcSender(event, rendererEntryPath);
    const result = await dialog.showOpenDialog({
      properties: ["openDirectory"],
      title: "Open Workspace",
    });

    return result.canceled ? undefined : result.filePaths[0];
  });

  ipcMain.handle("kosmos:window:minimize", (event) => {
    assertTrustedIpcSender(event, rendererEntryPath);
    BrowserWindow.fromWebContents(event.sender)?.minimize();
  });

  ipcMain.handle("kosmos:window:toggleMaximize", (event) => {
    assertTrustedIpcSender(event, rendererEntryPath);
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
    assertTrustedIpcSender(event, rendererEntryPath);
    BrowserWindow.fromWebContents(event.sender)?.close();
  });

  ipcMain.handle("kosmos:revealPath", (event, targetPath: unknown) => {
    assertTrustedIpcSender(event, rendererEntryPath);
    if (typeof targetPath !== "string" || targetPath.length === 0) {
      throw new Error("Reveal path must be a non-empty string");
    }

    shell.showItemInFolder(targetPath);
  });
}

function assertRequestAllowedDuringRecovery(
  request: KosmosIpcRequest,
  serverRecovery: ServerRecoveryState,
): void {
  if (!serverRecovery.active) {
    return;
  }
  if (
    request.domain === "editor" &&
    request.action === "restoreSession" &&
    serverRecovery.restoringRenderer
  ) {
    return;
  }
  if (
    request.domain === "editor" ||
    (request.domain === "tab" && (request.action === "close" || request.action === "resolveClose")) ||
    (request.domain === "workspace" &&
      (request.action === "close" ||
        request.action === "resolveClose" ||
        request.action === "closeApplication"))
  ) {
    throw new Error("Kosmos is restoring editor sessions after restarting its server");
  }
}

type IpcSender = {
  sender: WebContents;
  senderFrame: WebFrameMain | null;
};

export function isTrustedIpcSender(event: IpcSender, rendererEntryPath: string): boolean {
  return (
    event.senderFrame === event.sender.mainFrame &&
    isTrustedRendererUrl(event.senderFrame?.url ?? "", rendererEntryPath)
  );
}

function assertTrustedIpcSender(event: IpcSender, rendererEntryPath: string): void {
  if (!isTrustedIpcSender(event, rendererEntryPath)) {
    throw new Error("IPC request did not originate from the Kosmos renderer");
  }
}

function updateWindowSettingsPolicy(
  sender: WebContents,
  request: KosmosIpcRequest,
  result: unknown,
  settingsSnapshots: Map<number, SettingsSnapshot>,
): void {
  if (
    request.domain !== "settings" ||
    (request.action !== "get" && request.action !== "update") ||
    !isSettingsSnapshot(request.action, result)
  ) {
    return;
  }

  const window = BrowserWindow.fromWebContents(sender);
  if (!window || !updateWindowZoomPolicy(window, result)) {
    return;
  }

  settingsSnapshots.set(sender.id, result);
  setWindowZoomLevel(window, result.appearance.zoomLevel);
}

function beginRendererRequest(
  rendererRequests: Map<number, Map<string, AbortController>>,
  watchedRenderers: Set<number>,
  sender: WebContents,
  requestKey: string | undefined,
): AbortController | undefined {
  if (requestKey === undefined) {
    return undefined;
  }
  if (requestKey.length === 0 || requestKey.length > 128) {
    throw new Error("IPC request key must contain between 1 and 128 characters");
  }
  let requests = rendererRequests.get(sender.id);
  if (!requests) {
    requests = new Map();
    rendererRequests.set(sender.id, requests);
  }
  if (requests.has(requestKey)) {
    throw new Error("IPC request key is already active for this renderer");
  }
  if (!watchedRenderers.has(sender.id)) {
    watchedRenderers.add(sender.id);
    sender.once("destroyed", () => {
      for (const cancellation of rendererRequests.get(sender.id)?.values() ?? []) {
        cancellation.abort();
      }
      rendererRequests.delete(sender.id);
      watchedRenderers.delete(sender.id);
    });
  }
  const cancellation = new AbortController();
  requests.set(requestKey, cancellation);
  return cancellation;
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
