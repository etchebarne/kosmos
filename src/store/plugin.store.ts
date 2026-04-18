import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { getTauriStore } from "../lib/tauriStore";
import {
  registryEntryId,
  type InstalledPlugin,
  type PluginManifest,
  type RegistryEntry,
} from "../plugins/types";
import curatedRegistry from "../plugins/registry.json";

interface PluginStore {
  /** Map of plugin id → installed plugin state */
  plugins: Record<string, InstalledPlugin>;
  /** Curated registry entries fetched from remote */
  registry: RegistryEntry[];
  /** Whether initial scan has completed */
  ready: boolean;
  /** Plugin currently being installed (id) */
  installing: string | null;

  init: () => Promise<void>;
  scan: () => Promise<void>;
  fetchRegistry: () => Promise<void>;
  install: (entry: RegistryEntry) => Promise<void>;
  installFromUrl: (url: string) => Promise<void>;
  uninstall: (pluginId: string) => Promise<void>;
  setEnabled: (pluginId: string, enabled: boolean) => void;
  markActivated: (pluginId: string) => void;
}

async function loadDisabledSet(): Promise<Set<string>> {
  const s = await getTauriStore("plugins.json");
  const arr = await s.get<string[]>("disabled");
  return new Set(arr ?? []);
}

async function persistDisabledSet(disabled: Set<string>) {
  const s = await getTauriStore("plugins.json");
  await s.set("disabled", Array.from(disabled));
}

export const usePluginStore = create<PluginStore>((set, get) => ({
  plugins: {},
  registry: [],
  ready: false,
  installing: null,

  init: async () => {
    await get().scan();
    set({ ready: true });
    get().fetchRegistry();
  },

  scan: async () => {
    const manifests: Array<{ manifest: PluginManifest; path: string }> =
      await invoke("plugin_list");
    const disabled = await loadDisabledSet();
    const existing = get().plugins;

    const plugins: Record<string, InstalledPlugin> = {};
    for (const { manifest, path } of manifests) {
      // Directory name matches the registry-derived ID so marketplace installs keep their key.
      const dirName = path.split(/[\\/]/).pop() ?? manifest.name;
      plugins[dirName] = {
        pluginId: dirName,
        manifest,
        path,
        enabled: !disabled.has(dirName),
        activated: existing[dirName]?.activated ?? false,
      };
    }
    set({ plugins });
  },

  fetchRegistry: async () => {
    set({ registry: curatedRegistry as RegistryEntry[] });
  },

  install: async (entry) => {
    const id = registryEntryId(entry);
    set({ installing: id });
    try {
      await invoke("plugin_install", { url: entry.download, pluginId: id });
      await get().scan();
    } finally {
      set({ installing: null });
    }
  },

  installFromUrl: async (url) => {
    set({ installing: "unknown" });
    try {
      await invoke("plugin_install", { url, pluginId: null });
      await get().scan();
    } finally {
      set({ installing: null });
    }
  },

  uninstall: async (pluginId) => {
    await invoke("plugin_uninstall", { pluginId });
    const plugins = { ...get().plugins };
    delete plugins[pluginId];
    set({ plugins });
  },

  setEnabled: (pluginId, enabled) => {
    const plugin = get().plugins[pluginId];
    if (!plugin) return;
    const plugins = {
      ...get().plugins,
      [pluginId]: { ...plugin, enabled },
    };
    set({ plugins });

    const disabled = new Set<string>();
    for (const [id, p] of Object.entries(plugins)) {
      if (!p.enabled) disabled.add(id);
    }
    persistDisabledSet(disabled);
  },

  markActivated: (pluginId) => {
    const plugin = get().plugins[pluginId];
    if (!plugin) return;
    set({
      plugins: {
        ...get().plugins,
        [pluginId]: { ...plugin, activated: true },
      },
    });
  },
}));
