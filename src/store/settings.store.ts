import { create } from "zustand";
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
  },

  set: (key: string, value: unknown) => {
    const next = { ...get().values, [key]: value };
    set({ values: next });
    persist(next);
    if (key === "theme.colorTheme") {
      applyTheme(String(value));
    }
  },

  get: (key: string) => get().values[key],
}));
