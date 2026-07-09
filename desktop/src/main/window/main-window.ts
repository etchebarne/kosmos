import { app, BrowserWindow } from "electron";
import fs from "node:fs";
import path from "node:path";

import { registerWindowShortcuts } from "./shortcuts";

export function createMainWindow(): BrowserWindow {
  const runtimeDirectory = getRuntimeDirectory();
  const appIconPath = getAppIconPath();
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    title: "Kosmos",
    frame: false,
    autoHideMenuBar: true,
    backgroundColor: "#111217",
    icon: appIconPath,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(runtimeDirectory, "preload.cjs"),
      sandbox: false,
    },
  });

  registerWindowShortcuts(window);

  void window.loadFile(path.join(runtimeDirectory, "renderer", "index.html"));

  return window;
}

function getAppIconPath(): string {
  const assetsDirectory = app.isPackaged ? process.resourcesPath : app.getAppPath();

  return path.resolve(assetsDirectory, "assets", "icon", "icon-512.png");
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
