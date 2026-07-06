import { app, BrowserWindow, Menu, dialog, ipcMain, shell } from "electron";
import fs from "node:fs";
import path from "node:path";

import type { KosmosIpcDomain, KosmosIpcRequest } from "../shared/ipc";
import { KosmosServerClient } from "./server-client";

const validDomains = new Set<KosmosIpcDomain>(["workspace", "pane", "tab", "fileTree"]);
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

function createMainWindow(): void {
  const runtimeDirectory = getRuntimeDirectory();
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    title: "Kosmos",
    frame: false,
    autoHideMenuBar: true,
    backgroundColor: "#111217",
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(runtimeDirectory, "preload.cjs"),
      sandbox: false,
    },
  });

  registerWindowShortcuts(window);

  void window.loadFile(path.join(runtimeDirectory, "renderer", "index.html"));
}

function registerWindowShortcuts(window: BrowserWindow): void {
  window.webContents.on("before-input-event", (event, input) => {
    const key = input.key.toLowerCase();
    const togglesDevTools =
      input.key === "F12" || ((input.control || input.meta) && input.shift && key === "i");

    if (!togglesDevTools) {
      return;
    }

    event.preventDefault();

    if (window.webContents.isDevToolsOpened()) {
      window.webContents.closeDevTools();
    } else {
      window.webContents.openDevTools({ mode: "detach" });
    }
  });
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
  Menu.setApplicationMenu(null);
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
