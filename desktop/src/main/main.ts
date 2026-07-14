import {
  app,
  BrowserWindow,
  ipcMain,
  Menu,
  dialog,
  type IpcMainEvent,
  type MessageBoxOptions,
  type MessageBoxReturnValue,
} from "electron";

import { errorMessage } from "./error-message";
import {
  isTrustedIpcSender,
  registerIpcHandlers,
  type ApplyEditOwner,
} from "./ipc/handlers";
import { KosmosServerClient } from "./server/client";
import { KosmosServerProcess } from "./server/process";
import {
  createServerRecovery,
  type DegradedRecoveryState,
  type RecoveryChoice,
} from "./server/recovery";
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
const RENDERER_READY_TIMEOUT_MS = 10_000;
const applyEditOwners = new Map<string, ApplyEditOwner>();
const settingsSnapshots = new Map<number, SettingsSnapshot>();
const readyRenderers = new Set<number>();
let bypassShutdown = false;
let pendingDegradedRecovery: DegradedRecoveryState | undefined;
let recoveryDialogActive = false;
let lifecycleDialogTail = Promise.resolve();
const serverRecovery = createServerRecovery({
  now: Date.now,
  disconnectClient: () => serverClient.disconnect(),
  startServer: () => serverProcess.start(),
  reconnectClient: () => serverClient.reconnect(),
  requestRendererRestore,
  onDegraded: queueDegradedRecoveryDialog,
});
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
    const { response } = await showLifecycleMessageBox({
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

function handleUnexpectedServerExit(processToken: number, exitError: Error): void {
  if (bypassShutdown || shutdownAttempt.complete) {
    return;
  }

  serverRecovery.unexpectedExit(processToken, exitError);
}

function requestRendererRestore(generation: number): boolean {
  const windows = BrowserWindow.getAllWindows().filter(
    (window) => !window.webContents.isDestroyed(),
  );
  if (
    windows.length === 0 ||
    windows.some((window) => !readyRenderers.has(window.webContents.id))
  ) {
    return false;
  }
  for (const window of windows) {
    window.webContents.send("kosmos:serverReconnected", generation);
  }
  return true;
}

function queueDegradedRecoveryDialog(state: DegradedRecoveryState): void {
  pendingDegradedRecovery = state;
  void showDegradedRecoveryDialogs();
}

async function showDegradedRecoveryDialogs(): Promise<void> {
  if (recoveryDialogActive) {
    return;
  }
  recoveryDialogActive = true;
  try {
    while (pendingDegradedRecovery) {
      const degraded = pendingDegradedRecovery;
      pendingDegradedRecovery = undefined;
      if (
        serverRecovery.state.phase !== "degraded" ||
        serverRecovery.state.attemptToken !== degraded.attemptToken
      ) {
        continue;
      }
      const failure = degraded.failure.error.message.slice(0, 64 * 1024);
      const context =
        degraded.failure.stage === "renderer"
          ? "The server restarted, but editor sessions could not be restored."
          : "The server stopped and automatic recovery could not complete.";
      const { response } = await showLifecycleMessageBox({
        type: "error",
        title: "Kosmos server stopped",
        message: "Kosmos cannot currently use its server.",
        detail: `${context} Windows and in-memory editor buffers remain open for review.\n\n${failure}`,
        buttons: ["Retry Server", "Keep Kosmos Open", "Force Quit"],
        defaultId: 1,
        cancelId: 1,
      });
      const outcome = serverRecovery.resolveDegraded(
        degraded.attemptToken,
        recoveryChoice(response),
      );
      if (outcome === "forceQuit") {
        quitImmediately(1);
        return;
      }
    }
  } finally {
    recoveryDialogActive = false;
    if (pendingDegradedRecovery) {
      void showDegradedRecoveryDialogs();
    }
  }
}

async function showLifecycleMessageBox(
  options: MessageBoxOptions,
): Promise<MessageBoxReturnValue> {
  const previous = lifecycleDialogTail;
  let release!: () => void;
  lifecycleDialogTail = new Promise<void>((resolve) => {
    release = resolve;
  });
  await previous;
  try {
    return await dialog.showMessageBox(options);
  } finally {
    release();
  }
}

function recoveryChoice(response: number): RecoveryChoice {
  switch (response) {
    case 0:
      return "retry";
    case 2:
      return "forceQuit";
    default:
      return "keepOpen";
  }
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
