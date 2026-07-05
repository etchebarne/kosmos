import { app, BrowserWindow, dialog, ipcMain } from "electron";
import fs from "node:fs";
import path from "node:path";

import type { KosmosIpcDomain, KosmosIpcRequest } from "../shared/ipc";
import { KosmosServerClient } from "./server-client";

const validDomains = new Set<KosmosIpcDomain>(["workspace", "pane", "tab"]);
const serverClient = new KosmosServerClient();

function registerIpcHandlers(): void {
  ipcMain.handle("kosmos:request", (_event, request: KosmosIpcRequest) => {
    validateRequest(request);
    return serverClient.request(request.domain, request.action, request.params ?? {});
  });

  ipcMain.handle("kosmos:socketPath", () => serverClient.socketPath);

  ipcMain.handle("kosmos:selectWorkspaceDirectory", async () => {
    const result = await dialog.showOpenDialog({
      properties: ["openDirectory"],
      title: "Open Workspace",
    });

    return result.canceled ? undefined : result.filePaths[0];
  });
}

function createMainWindow(): void {
  const runtimeDirectory = getRuntimeDirectory();
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    title: "Kosmos",
    backgroundColor: "#111217",
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(runtimeDirectory, "preload.cjs"),
      sandbox: false,
    },
  });

  void window.loadFile(path.join(runtimeDirectory, "renderer", "index.html"));
}

function getRuntimeDirectory(): string {
  const packageJsonPath = path.join(app.getAppPath(), "package.json");
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8")) as {
    main?: unknown;
  };

  if (typeof packageJson.main !== "string" || packageJson.main.length === 0) {
    throw new Error("package.json must define the Electron main entry");
  }

  return path.dirname(path.resolve(app.getAppPath(), packageJson.main));
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

app.whenReady().then(() => {
  registerIpcHandlers();
  createMainWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createMainWindow();
    }
  });
});

app.on("window-all-closed", () => {
  serverClient.disconnect();

  if (process.platform !== "darwin") {
    app.quit();
  }
});
