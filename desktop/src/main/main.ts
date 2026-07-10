import { app, BrowserWindow, ipcMain, Menu, dialog, type IpcMainEvent } from "electron";

import { errorMessage } from "./error-message";
import { registerIpcHandlers } from "./ipc/handlers";
import { KosmosServerClient } from "./server/client";
import { KosmosServerProcess } from "./server/process";
import { createMainWindow } from "./window/main-window";

const serverClient = new KosmosServerClient();
const serverProcess = new KosmosServerProcess(serverClient.socketPath);
const RENDERER_FLUSH_TIMEOUT_MS = 1_000;
const SERVER_SHUTDOWN_TIMEOUT_MS = 5_000;
let shutdownComplete = false;
let shutdownStarted = false;

app.commandLine.appendSwitch("use-webgpu-adapter", "opengles");

app.on("browser-window-created", (_event, window) => {
  let rendererFlushed = false;
  let rendererFlushStarted = false;

  window.on("close", (event) => {
    if (shutdownComplete || rendererFlushed) {
      return;
    }

    event.preventDefault();
    if (rendererFlushStarted) {
      return;
    }

    rendererFlushStarted = true;
    void flushRendererState(window).finally(() => {
      rendererFlushed = true;

      if (!window.isDestroyed()) {
        window.close();
      }
    });
  });
});

async function startApp(): Promise<void> {
  Menu.setApplicationMenu(null);
  await serverProcess.start();
  registerIpcHandlers(serverClient);
  serverClient.onWorkspaceChanged((workspaceIds) => {
    for (const window of BrowserWindow.getAllWindows()) {
      if (!window.webContents.isDestroyed()) {
        window.webContents.send("kosmos:workspaceChanged", workspaceIds);
      }
    }
  });
  createMainWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createMainWindow();
    }
  });
}

app.whenReady().then(startApp).catch((error: unknown) => {
  dialog.showErrorBox("Kosmos failed to start", errorMessage(error));
  app.quit();
});

app.on("before-quit", (event) => {
  if (shutdownComplete) {
    return;
  }

  event.preventDefault();

  if (shutdownStarted) {
    return;
  }

  shutdownStarted = true;

  void flushAndStopServer().finally(() => {
    shutdownComplete = true;
    app.quit();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

async function flushAndStopServer(): Promise<void> {
  try {
    await flushRendererState();
    await Promise.race([
      serverClient.flushPersistence(),
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("Server shutdown timed out")), SERVER_SHUTDOWN_TIMEOUT_MS),
      ),
    ]);
  } catch {
    // Shutdown still needs to proceed if the sidecar is already unavailable.
  } finally {
    serverClient.disconnect();
    serverProcess.stop();
  }
}

function flushRendererState(targetWindow?: BrowserWindow): Promise<void> {
  const window = targetWindow ?? BrowserWindow.getAllWindows()[0];
  if (!window || window.webContents.isDestroyed()) {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    const finish = () => {
      clearTimeout(timeout);
      ipcMain.off("kosmos:rendererStateFlushed", onFlushed);
      resolve();
    };
    const onFlushed = (event: IpcMainEvent) => {
      if (event.sender === window.webContents) {
        finish();
      }
    };
    const timeout = setTimeout(finish, RENDERER_FLUSH_TIMEOUT_MS);

    ipcMain.on("kosmos:rendererStateFlushed", onFlushed);
    window.webContents.send("kosmos:flushState");
  });
}
