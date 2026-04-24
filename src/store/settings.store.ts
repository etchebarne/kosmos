import { create } from "zustand";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getTauriStore } from "../lib/tauriStore";
import { applyTheme } from "../lib/themes";

interface SettingsStore {
  values: Record<string, unknown>;
  ready: boolean;

  init: () => Promise<void>;
  set: (key: string, value: unknown) => void;
  get: (key: string) => unknown;
}

async function persist(values: Record<string, unknown>) {
  const s = await getTauriStore("settings.json");
  await s.set("values", values);
}

function applySolidMode(enabled: boolean) {
  document.documentElement.setAttribute("data-solid", enabled ? "true" : "false");
}

function applyUiZoom(percent: unknown) {
  const n = Number(percent);
  const clamped = Number.isFinite(n) ? Math.min(125, Math.max(80, n)) : 100;
  getCurrentWebview()
    .setZoom(clamped / 100)
    .catch((err) => console.warn("Failed to set webview zoom:", err));
}

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  values: {},
  ready: false,

  init: async () => {
    const s = await getTauriStore("settings.json");
    const values = (await s.get<Record<string, unknown>>("values")) ?? {};
    set({ values, ready: true });

    const colorTheme = values["theme.colorTheme"];
    if (colorTheme !== undefined) {
      applyTheme(String(colorTheme));
    }
    applySolidMode(values["theme.solidMode"] === true);
    applyUiZoom(values["theme.uiZoom"] ?? 100);
  },

  set: (key: string, value: unknown) => {
    const next = { ...get().values, [key]: value };
    set({ values: next });
    persist(next);
    if (key === "theme.colorTheme") {
      applyTheme(String(value));
    } else if (key === "theme.solidMode") {
      applySolidMode(value === true);
    } else if (key === "theme.uiZoom") {
      applyUiZoom(value);
    }
  },

  get: (key: string) => get().values[key],
}));
