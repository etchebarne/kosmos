import { app, BrowserWindow, screen, type Rectangle } from "electron";
import fs from "node:fs";
import path from "node:path";

import type { SettingsSnapshot, WindowState } from "../../shared/ipc";
import type { KosmosServerClient } from "../server/client";
import {
  registerWindowShortcuts,
  setWindowZoomLevel,
  windowZoomPolicy,
} from "./shortcuts";
import { isSettingsSnapshot } from "../settings-snapshot";

const DEFAULT_WINDOW_BOUNDS = { width: 1280, height: 800 };
const WINDOW_STATE_SAVE_DELAY_MS = 250;

export async function createMainWindow(
  serverClient: KosmosServerClient,
  settings: SettingsSnapshot,
  onSettingsSnapshot: (window: BrowserWindow, snapshot: SettingsSnapshot) => void,
): Promise<BrowserWindow> {
  const runtimeDirectory = getRuntimeDirectory();
  const appIconPath = getAppIconPath();
  const state = await loadWindowState(serverClient);
  const window = new BrowserWindow({
    x: state.x,
    y: state.y,
    width: state.width,
    height: state.height,
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

  registerWindowShortcuts(window, settings, (zoomLevel) => {
    void persistShortcutZoom(serverClient, window, zoomLevel, onSettingsSnapshot);
  });
  setWindowZoomLevel(window, settings.appearance.zoomLevel);
  registerWindowStatePersistence(window, serverClient);

  if (state.maximized) {
    window.maximize();
  }
  if (state.fullscreen) {
    window.setFullScreen(true);
  }

  void window.loadFile(path.join(runtimeDirectory, "renderer", "index.html"));

  return window;
}

async function persistShortcutZoom(
  serverClient: KosmosServerClient,
  window: BrowserWindow,
  zoomLevel: number,
  onSettingsSnapshot: (window: BrowserWindow, snapshot: SettingsSnapshot) => void,
): Promise<void> {
  try {
    const policy = windowZoomPolicy(window);
    const snapshot = await serverClient.request<unknown>("settings", "update", {
      id: policy.appearance.zoomSettingId,
      value: zoomLevel,
    });
    if (!isSettingsSnapshot("update", snapshot)) {
      throw new Error("Invalid settings.update result from server");
    }

    onSettingsSnapshot(window, snapshot);
  } catch {
    setWindowZoomLevel(window, windowZoomPolicy(window).appearance.zoomLevel);
  }
}

async function loadWindowState(serverClient: KosmosServerClient): Promise<WindowState> {
  const primaryWorkArea = screen.getPrimaryDisplay().workArea;
  const fallback: WindowState = {
    x: primaryWorkArea.x + Math.round((primaryWorkArea.width - DEFAULT_WINDOW_BOUNDS.width) / 2),
    y: primaryWorkArea.y + Math.round((primaryWorkArea.height - DEFAULT_WINDOW_BOUNDS.height) / 2),
    ...DEFAULT_WINDOW_BOUNDS,
    fullscreen: false,
    maximized: false,
  };

  try {
    const state = await serverClient.request<unknown>("window", "get");
    if (!isWindowState(state) || !isVisibleOnAnyDisplay(state)) {
      return fallback;
    }

    return state;
  } catch {
    return fallback;
  }
}

function registerWindowStatePersistence(
  window: BrowserWindow,
  serverClient: KosmosServerClient,
): void {
  let saveTimeout: NodeJS.Timeout | undefined;
  const save = () => {
    if (window.isDestroyed()) {
      return;
    }

    const bounds = window.getNormalBounds();
    void serverClient
      .request("window", "update", {
        ...bounds,
        fullscreen: window.isFullScreen(),
        maximized: window.isMaximized(),
      } satisfies WindowState)
      .catch(() => undefined);
  };
  const scheduleSave = () => {
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(save, WINDOW_STATE_SAVE_DELAY_MS);
  };

  window.on("move", scheduleSave);
  window.on("resize", scheduleSave);
  window.on("maximize", scheduleSave);
  window.on("unmaximize", scheduleSave);
  window.on("enter-full-screen", scheduleSave);
  window.on("leave-full-screen", scheduleSave);
  window.on("close", () => {
    clearTimeout(saveTimeout);
    save();
  });
}

function isWindowState(value: unknown): value is WindowState {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  const state = value as Partial<WindowState>;
  return (
    typeof state.fullscreen === "boolean" &&
    typeof state.maximized === "boolean" &&
    Number.isSafeInteger(state.x) &&
    Number.isSafeInteger(state.y) &&
    Number.isSafeInteger(state.width) &&
    Number.isSafeInteger(state.height) &&
    state.width! >= 900 &&
    state.height! >= 600
  );
}

function isVisibleOnAnyDisplay(bounds: Rectangle): boolean {
  return screen.getAllDisplays().some((display) => {
    const area = display.workArea;
    return (
      bounds.x < area.x + area.width &&
      bounds.x + bounds.width > area.x &&
      bounds.y < area.y + area.height &&
      bounds.y + bounds.height > area.y
    );
  });
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
