import { app, BrowserWindow, ipcMain, Menu, dialog, type IpcMainEvent } from "electron";

import { errorMessage } from "./error-message";
import {
  isTrustedIpcSender,
  registerIpcHandlers,
  type ApplyEditOwner,
} from "./ipc/handlers";
import { KosmosServerClient } from "./server/client";
import { KosmosServerProcess } from "./server/process";
import { loadBootstrapSettings, newerSettingsSnapshot } from "./settings-snapshot";
import { createShutdownAttempt } from "./shutdown-attempt";
import { startWithFatalHandler } from "./startup";
import { createMainWindow, getRendererEntryPath } from "./window/main-window";
import { setWindowZoomLevel, updateWindowZoomPolicy } from "./window/shortcuts";
import type { SettingsSnapshot } from "../shared/ipc";
import { configureDevelopmentInstance } from "./development-instance";

configureDevelopmentInstance(app);

const serverClient = new KosmosServerClient();
const serverProcess = new KosmosServerProcess(serverClient.socketPath, handleUnexpectedServerExit);
const SERVER_SHUTDOWN_TIMEOUT_MS = 5_000;
const MAX_SIDECAR_RESTARTS = 3;
const SIDECAR_RESTART_WINDOW_MS = 60_000;
const RENDERER_READY_TIMEOUT_MS = 10_000;
const applyEditOwners = new Map<string, ApplyEditOwner>();
const settingsSnapshots = new Map<number, SettingsSnapshot>();
const readyRenderers = new Set<number>();
const serverRecovery = { active: false, generation: 0 };
let bypassShutdown = false;
let sidecarRestart: Promise<void> | undefined;
const sidecarRestartTimes: number[] = [];
const shutdownAttempt = createShutdownAttempt(async () => {
  await flushAndStopServer();
  serverClient.disconnect();
  serverProcess.stop();
});

app.commandLine.appendSwitch("use-webgpu-adapter", "opengles");

const hasSingleInstanceLock = app.requestSingleInstanceLock();

if (hasSingleInstanceLock) {
  app.on("second-instance", focusMainWindow);
  void app.whenReady().then(() =>
    startWithFatalHandler(startApp, (error: unknown) => {
      dialog.showErrorBox("Kosmos failed to start", errorMessage(error));
      quitImmediately();
    }),
  );
} else {
  quitImmediately(0);
}

async function startApp(): Promise<void> {
  Menu.setApplicationMenu(null);
  await serverProcess.start();
  registerIpcHandlers(
    serverClient,
    applyEditOwners,
    settingsSnapshots,
    getRendererEntryPath(),
    readyRenderers,
    serverRecovery,
  );
  serverClient.onWorkspaceChanged((workspaceIds) => {
    for (const window of BrowserWindow.getAllWindows()) {
      if (!window.webContents.isDestroyed()) {
        window.webContents.send("kosmos:workspaceChanged", workspaceIds);
      }
    }
  });
  serverClient.onNotification((notification) => {
    if (notification.event === "languageServerApplyEdit") {
      const window = BrowserWindow.getFocusedWindow() ?? BrowserWindow.getAllWindows()[0];
      if (!window || window.webContents.isDestroyed()) {
        void serverClient.acknowledgeApplyEdit(
          notification.id,
          notification.token,
          false,
          "No renderer window is available to apply the workspace edit.",
        );
        return;
      }
      const id = notification.id;
      const token = notification.token;
      const webContents = window.webContents;
      const onDestroyed = () => {
        const owner = applyEditOwners.get(token);
        if (owner?.id !== id) {
          return;
        }
        applyEditOwners.delete(token);
        void serverClient.acknowledgeApplyEdit(
          id,
          token,
          false,
          "Owning renderer window disconnected while applying the workspace edit.",
        );
      };
      applyEditOwners.set(notification.token, {
        id,
        webContentsId: webContents.id,
        notification,
        cleanup: () => webContents.removeListener("destroyed", onDestroyed),
      });
      webContents.once("destroyed", onDestroyed);
      webContents.send("kosmos:serverNotification", notification);
      return;
    }
    if (notification.event === "languageServerApplyEditCancelled") {
      const owner = applyEditOwners.get(notification.token);
      const window = owner
        ? BrowserWindow.getAllWindows().find(
            (candidate) => candidate.webContents.id === owner.webContentsId,
          )
        : undefined;
      if (window && !window.webContents.isDestroyed()) {
        window.webContents.send("kosmos:serverNotification", notification);
      }
      return;
    }
    for (const window of BrowserWindow.getAllWindows()) {
      if (!window.webContents.isDestroyed()) {
        window.webContents.send("kosmos:serverNotification", notification);
      }
    }
  });
  serverClient.onReconnected(() => {
    for (const window of BrowserWindow.getAllWindows()) {
      if (!window.webContents.isDestroyed()) {
        window.webContents.send("kosmos:serverReconnected", serverRecovery.generation);
      }
    }
  });
  await createWindowWithSettings();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      void createWindowWithSettings().catch((error: unknown) => {
        dialog.showErrorBox("Kosmos failed to create a window", errorMessage(error));
      });
    }
  });
}

async function createWindowWithSettings(): Promise<BrowserWindow> {
  const settings = await fetchBootstrapSettings();
  return createMainWindow(
    serverClient,
    settings,
    (window) => {
      const webContentsId = window.webContents.id;
      settingsSnapshots.set(webContentsId, settings);
      window.once("closed", () => settingsSnapshots.delete(webContentsId));
      window.on("close", (event) => {
        if (shutdownAttempt.complete || bypassShutdown) {
          return;
        }

        event.preventDefault();
        void beginShutdown(window);
      });
    },
    applySettingsSnapshot,
  );
}

async function fetchBootstrapSettings(): Promise<SettingsSnapshot> {
  return loadBootstrapSettings(() => serverClient.request<unknown>("settings", "get"));
}

function applySettingsSnapshot(window: BrowserWindow, snapshot: SettingsSnapshot): void {
  const previous = settingsSnapshots.get(window.webContents.id);
  if (!newerSettingsSnapshot(previous, snapshot)) {
    return;
  }

  settingsSnapshots.set(window.webContents.id, snapshot);
  if (updateWindowZoomPolicy(window, snapshot)) {
    setWindowZoomLevel(window, snapshot.appearance.zoomLevel);
  }
  if (!window.webContents.isDestroyed()) {
    window.webContents.send("kosmos:settingsSnapshot", snapshot);
  }
}

app.on("before-quit", (event) => {
  if (shutdownAttempt.complete || bypassShutdown) {
    return;
  }

  event.preventDefault();

  void beginShutdown();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

async function beginShutdown(window?: BrowserWindow): Promise<void> {
  const outcome = await shutdownAttempt.attempt(() => resolveRendererShutdown(window));
  if (outcome === "completed") {
    app.quit();
    return;
  }
  if (outcome === "failed") {
    const { response } = await dialog.showMessageBox({
      type: "error",
      title: "Kosmos is still running",
      message: "Shutdown could not be completed.",
      detail: "Keep Kosmos open to protect unsaved work, or force it to exit.",
      buttons: ["Keep Running", "Force Quit"],
      defaultId: 0,
      cancelId: 0,
    });
    if (response === 1) {
      quitImmediately(1);
    }
  }
}

async function flushAndStopServer(): Promise<void> {
  await Promise.race([
    serverClient.flushPersistence(),
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error("Server shutdown timed out")), SERVER_SHUTDOWN_TIMEOUT_MS),
    ),
  ]);
}

function resolveRendererShutdown(targetWindow?: BrowserWindow): Promise<boolean> {
  const window = targetWindow ?? BrowserWindow.getAllWindows()[0];
  if (!window || window.webContents.isDestroyed()) {
    return Promise.reject(new Error("Renderer is unavailable to resolve unsaved document changes."));
  }

  return new Promise((resolve, reject) => {
    let readyTimeout: ReturnType<typeof setTimeout> | undefined;
    const finish = () => {
      clearTimeout(readyTimeout);
      ipcMain.off("kosmos:shutdownResolved", onResolved);
      window.webContents.off("destroyed", onUnavailable);
      window.webContents.off("render-process-gone", onUnavailable);
      window.webContents.off("did-finish-load", sendRequest);
    };
    const onUnavailable = () => {
      finish();
      reject(new Error("Renderer stopped before resolving unsaved document changes."));
    };
    const onResolved = (event: IpcMainEvent, result: unknown) => {
      if (
        event.sender !== window.webContents ||
        !isTrustedIpcSender(event, getRendererEntryPath())
      ) {
        return;
      }
      finish();
      if (
        result &&
        typeof result === "object" &&
        "approved" in result &&
        result.approved === true
      ) {
        resolve(true);
      } else if (
        result &&
        typeof result === "object" &&
        "error" in result &&
        typeof result.error === "string"
      ) {
        reject(new Error(result.error));
      } else {
        resolve(false);
      }
    };
    const sendRequest = () => {
      if (window.webContents.isDestroyed()) {
        onUnavailable();
        return;
      }
      window.webContents.send("kosmos:prepareShutdown");
      if (!readyRenderers.has(window.webContents.id)) {
        readyTimeout = setTimeout(() => {
          if (readyRenderers.has(window.webContents.id)) {
            return;
          }
          finish();
          reject(new Error("Renderer did not become ready to resolve shutdown."));
        }, RENDERER_READY_TIMEOUT_MS);
      }
    };
    ipcMain.on("kosmos:shutdownResolved", onResolved);
    window.webContents.once("destroyed", onUnavailable);
    window.webContents.once("render-process-gone", onUnavailable);
    if (window.webContents.isLoadingMainFrame()) {
      window.webContents.once("did-finish-load", sendRequest);
    } else {
      sendRequest();
    }
  });
}

function focusMainWindow(): void {
  const window = BrowserWindow.getAllWindows()[0];
  if (!window) {
    return;
  }
  if (window.isMinimized()) {
    window.restore();
  }
  window.show();
  window.focus();
}

function handleUnexpectedServerExit(exitError: Error): void {
  if (bypassShutdown || shutdownAttempt.complete || sidecarRestart) {
    return;
  }

  serverClient.disconnect();
  serverRecovery.generation += 1;
  serverRecovery.active = true;
  const now = Date.now();
  while (sidecarRestartTimes[0] && now - sidecarRestartTimes[0] > SIDECAR_RESTART_WINDOW_MS) {
    sidecarRestartTimes.shift();
  }
  if (sidecarRestartTimes.length >= MAX_SIDECAR_RESTARTS) {
    dialog.showErrorBox(
      "Kosmos server stopped",
      `The Kosmos server repeatedly stopped and the application must close.\n\n${exitError.message}`,
    );
    quitImmediately(1);
    return;
  }
  sidecarRestartTimes.push(now);

  sidecarRestart = serverProcess
    .start()
    .then(() => serverClient.reconnect())
    .catch((error: unknown) => {
      dialog.showErrorBox("Kosmos server could not restart", errorMessage(error));
      quitImmediately(1);
    })
    .finally(() => {
      sidecarRestart = undefined;
    });
}

function quitImmediately(exitCode = 1): void {
  bypassShutdown = true;
  serverClient.disconnect();
  try {
    serverProcess.stop();
  } finally {
    app.exit(exitCode);
  }
}
