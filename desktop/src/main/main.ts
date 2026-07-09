import { app, BrowserWindow, Menu, dialog } from "electron";

import { errorMessage } from "./error-message";
import { registerIpcHandlers } from "./ipc/handlers";
import { KosmosServerClient } from "./server/client";
import { KosmosServerProcess } from "./server/process";
import { createMainWindow } from "./window/main-window";

const serverClient = new KosmosServerClient();
const serverProcess = new KosmosServerProcess(serverClient.socketPath);

app.commandLine.appendSwitch("use-webgpu-adapter", "opengles");

async function startApp(): Promise<void> {
  Menu.setApplicationMenu(null);
  await serverProcess.start();
  registerIpcHandlers(serverClient);
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

app.on("before-quit", () => {
  serverClient.disconnect();
  serverProcess.stop();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});
