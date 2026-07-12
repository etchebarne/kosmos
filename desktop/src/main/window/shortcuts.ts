import type { BrowserWindow, Input } from "electron";

import type { ResolvedAppearanceSettings, SettingsSnapshot } from "../../shared/ipc";

export type WindowZoomPolicyCache = {
  revision: number;
  appearance: ResolvedAppearanceSettings;
};

const windowZoomPolicies = new WeakMap<BrowserWindow, WindowZoomPolicyCache>();

export function registerWindowShortcuts(
  window: BrowserWindow,
  snapshot: SettingsSnapshot,
  onZoomChanged: (zoomLevel: number) => void,
): void {
  const policy = createWindowZoomPolicyCache(snapshot);
  windowZoomPolicies.set(window, policy);
  window.webContents.on("before-input-event", (event, input) => {
    if (input.type !== "keyDown") {
      return;
    }

    const zoomAction = uiZoomShortcutAction(input);

    if (zoomAction) {
      event.preventDefault();
      onZoomChanged(handleWindowZoom(window, policy, zoomAction));
      return;
    }

    if (!togglesDevTools(input)) {
      return;
    }

    event.preventDefault();
    toggleDevTools(window);
  });
}

export function createWindowZoomPolicyCache(snapshot: SettingsSnapshot): WindowZoomPolicyCache {
  return {
    revision: snapshot.revision,
    appearance: snapshot.appearance,
  };
}

export function updateWindowZoomPolicyCache(
  policy: WindowZoomPolicyCache,
  snapshot: SettingsSnapshot,
): boolean {
  if (snapshot.revision <= policy.revision) {
    return false;
  }

  policy.revision = snapshot.revision;
  policy.appearance = snapshot.appearance;
  return true;
}

export function updateWindowZoomPolicy(window: BrowserWindow, snapshot: SettingsSnapshot): boolean {
  const policy = windowZoomPolicies.get(window);
  return policy ? updateWindowZoomPolicyCache(policy, snapshot) : false;
}

export function windowZoomPolicy(window: BrowserWindow): WindowZoomPolicyCache {
  const policy = windowZoomPolicies.get(window);
  if (!policy) {
    throw new Error("Window zoom policy is unavailable");
  }

  return policy;
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
  const policy = windowZoomPolicy(window);
  window.webContents.setZoomFactor(clampZoomLevel(zoomLevel, policy.appearance) / 100);
}

export function handleWindowZoom(
  window: BrowserWindow,
  policy: WindowZoomPolicyCache,
  action: "in" | "out" | "reset",
): number {
  if (action === "reset") {
    return setZoomLevel(window, policy.appearance.defaultZoomLevel, policy.appearance);
  }

  const direction = action === "in" ? 1 : -1;
  const currentZoomLevel = window.webContents.getZoomFactor() * 100;
  return setZoomLevel(
    window,
    currentZoomLevel + policy.appearance.zoomStep * direction,
    policy.appearance,
  );
}

function setZoomLevel(
  window: BrowserWindow,
  zoomLevel: number,
  appearance: ResolvedAppearanceSettings,
): number {
  const nextZoomLevel = clampZoomLevel(zoomLevel, appearance);
  window.webContents.setZoomFactor(nextZoomLevel / 100);
  return nextZoomLevel;
}

function clampZoomLevel(zoomLevel: number, appearance: ResolvedAppearanceSettings): number {
  const roundedZoomLevel = Math.round(zoomLevel * 100) / 100;

  return Math.min(
    appearance.maxZoomLevel,
    Math.max(appearance.minZoomLevel, roundedZoomLevel),
  );
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
