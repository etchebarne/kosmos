import { app, BrowserWindow, ipcMain, Menu, dialog, type IpcMainEvent } from "electron";

import { errorMessage } from "./error-message";
import { registerIpcHandlers, type ApplyEditOwner } from "./ipc/handlers";
import { KosmosServerClient } from "./server/client";
import { KosmosServerProcess } from "./server/process";
import { loadBootstrapSettings, newerSettingsSnapshot } from "./settings-snapshot";
import { createShutdownAttempt } from "./shutdown-attempt";
import { startWithFatalHandler } from "./startup";
import { createMainWindow } from "./window/main-window";
import { setWindowZoomLevel, updateWindowZoomPolicy } from "./window/shortcuts";
import type { SettingsSnapshot } from "../shared/ipc";

const serverClient = new KosmosServerClient();
const serverProcess = new KosmosServerProcess(serverClient.socketPath);
const RENDERER_FLUSH_TIMEOUT_MS = 1_000;
const SERVER_SHUTDOWN_TIMEOUT_MS = 5_000;
const applyEditOwners = new Map<string, ApplyEditOwner>();
const settingsSnapshots = new Map<number, SettingsSnapshot>();
const shutdownAttempt = createShutdownAttempt(async () => {
  await flushAndStopServer();
  serverClient.disconnect();
  serverProcess.stop();
});

app.commandLine.appendSwitch("use-webgpu-adapter", "opengles");

app.on("browser-window-created", (_event, window) => {
  window.on("close", (event) => {
    if (shutdownAttempt.complete) {
      return;
    }

    event.preventDefault();
    void beginShutdown(window);
  });
});

async function startApp(): Promise<void> {
  Menu.setApplicationMenu(null);
  await serverProcess.start();
  registerIpcHandlers(serverClient, applyEditOwners, settingsSnapshots);
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
        window.webContents.send("kosmos:serverReconnected");
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
  const window = await createMainWindow(serverClient, settings, applySettingsSnapshot);
  settingsSnapshots.set(window.webContents.id, settings);
  window.once("closed", () => settingsSnapshots.delete(window.webContents.id));
  return window;
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

void app.whenReady().then(() =>
  startWithFatalHandler(startApp, (error: unknown) => {
    dialog.showErrorBox("Kosmos failed to start", errorMessage(error));
    app.quit();
  }),
);

app.on("before-quit", (event) => {
  if (shutdownAttempt.complete) {
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
    dialog.showErrorBox("Kosmos is still running", "Shutdown could not be completed. Try again.");
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
    const finish = () => {
      clearTimeout(timeout);
      ipcMain.off("kosmos:shutdownResolved", onResolved);
    };
    const onResolved = (event: IpcMainEvent, result: unknown) => {
      if (event.sender !== window.webContents) {
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
    const timeout = setTimeout(() => {
      finish();
      reject(new Error("Renderer shutdown approval timed out"));
    }, RENDERER_FLUSH_TIMEOUT_MS);

    ipcMain.on("kosmos:shutdownResolved", onResolved);
    window.webContents.send("kosmos:prepareShutdown");
  });
}
