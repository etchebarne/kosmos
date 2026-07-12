import { BrowserWindow, dialog, ipcMain, shell, type WebContents } from "electron";

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
import { ipcRequestFailure } from "./request-result";

export type ApplyEditOwner = {
  id: number;
  webContentsId: number;
  notification: Extract<KosmosServerNotification, { event: "languageServerApplyEdit" }>;
  cleanup(): void;
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
): void {
  const rendererRequests = new Map<number, Map<string, AbortController>>();
  const watchedRenderers = new Set<number>();

  ipcMain.on("kosmos:cancelRequest", (event, requestKey: unknown) => {
    if (typeof requestKey !== "string") {
      return;
    }
    rendererRequests.get(event.sender.id)?.get(requestKey)?.abort();
  });
  ipcMain.on("kosmos:serverApplyEditAck", (event, acknowledgement: unknown) => {
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
      validateRequest(request);
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
  ipcMain.handle("kosmos:pendingServerApplyEdits", (event) =>
    [...applyEditOwners.values()]
      .filter((owner) => owner.webContentsId === event.sender.id)
      .map((owner) => owner.notification),
  );
  ipcMain.handle("kosmos:bootstrapSettings", (event) => {
    const snapshot = settingsSnapshots.get(event.sender.id);
    if (!snapshot) {
      throw new Error("Bootstrap settings are unavailable for this window");
    }

    return snapshot;
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
