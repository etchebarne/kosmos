import type { BrowserWindow, Input } from "electron";

const MIN_UI_ZOOM_FACTOR = 0.8;
const MAX_UI_ZOOM_FACTOR = 1.4;
const UI_ZOOM_STEP = 0.1;
const DEFAULT_UI_ZOOM_FACTOR = 1;

export function registerWindowShortcuts(window: BrowserWindow): void {
  window.webContents.on("before-input-event", (event, input) => {
    if (input.type !== "keyDown") {
      return;
    }

    const zoomAction = uiZoomShortcutAction(input);

    if (zoomAction) {
      event.preventDefault();
      const zoomLevel = handleWindowZoom(window, zoomAction);
      window.webContents.send("kosmos:window:zoomLevelChanged", zoomLevel);
      return;
    }

    if (!togglesDevTools(input)) {
      return;
    }

    event.preventDefault();
    toggleDevTools(window);
  });
}

function uiZoomShortcutAction(input: Input): "in" | "out" | "reset" | undefined {
  if (!input.control && !input.meta) {
    return undefined;
  }

  const key = input.key.toLowerCase();

  if (key === "0" || input.code === "Digit0" || input.code === "Numpad0") {
    return "reset";
  }

  if (key === "+" || key === "=" || input.code === "Equal" || input.code === "NumpadAdd") {
    return "in";
  }

  if (key === "-" || input.code === "Minus" || input.code === "NumpadSubtract") {
    return "out";
  }

  return undefined;
}

export function setWindowZoomLevel(window: BrowserWindow, zoomLevel: number): void {
  window.webContents.setZoomFactor(clampUiZoomFactor(zoomLevel / 100));
}

function handleWindowZoom(window: BrowserWindow, action: "in" | "out" | "reset"): number {
  if (action === "reset") {
    window.webContents.setZoomFactor(DEFAULT_UI_ZOOM_FACTOR);
    return DEFAULT_UI_ZOOM_FACTOR * 100;
  }

  return adjustWindowZoom(window, action === "in" ? UI_ZOOM_STEP : -UI_ZOOM_STEP) * 100;
}

function adjustWindowZoom(window: BrowserWindow, delta: number): number {
  const currentZoomFactor = window.webContents.getZoomFactor();
  const nextZoomFactor = clampUiZoomFactor(currentZoomFactor + delta);

  window.webContents.setZoomFactor(nextZoomFactor);
  return nextZoomFactor;
}

function clampUiZoomFactor(zoomFactor: number): number {
  const roundedZoomFactor = Math.round(zoomFactor * 100) / 100;

  return Math.min(MAX_UI_ZOOM_FACTOR, Math.max(MIN_UI_ZOOM_FACTOR, roundedZoomFactor));
}

function togglesDevTools(input: Input): boolean {
  const key = input.key.toLowerCase();

  return input.key === "F12" || ((input.control || input.meta) && input.shift && key === "i");
}

function toggleDevTools(window: BrowserWindow): void {
  if (window.webContents.isDevToolsOpened()) {
    window.webContents.closeDevTools();
  } else {
    window.webContents.openDevTools({ mode: "detach" });
  }
}
