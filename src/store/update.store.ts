import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";

interface UpdateStore {
  /** The available update, or null if up to date */
  update: Update | null;
  /** Whether an update is currently being downloaded/installed */
  installing: boolean;
  /** Check GitHub for a newer release */
  checkForUpdate: () => Promise<void>;
  /** Download, install, and relaunch */
  installUpdate: () => Promise<void>;
}

export const useUpdateStore = create<UpdateStore>((set, get) => ({
  update: null,
  installing: false,

  checkForUpdate: async () => {
    try {
      // Linux updater only supports AppImage; deb/AUR go via package manager.
      const isLinux = navigator.userAgent.includes("Linux");
      if (isLinux && !(await invoke<boolean>("is_appimage"))) return;

      const update = await check();
      set({ update: update?.available ? update : null });
    } catch (e) {
      console.warn("Update check failed:", e);
    }
  },

  installUpdate: async () => {
    const { update } = get();
    if (!update || get().installing) return;
    set({ installing: true });
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      console.error("Update install failed:", e);
      set({ installing: false });
    }
  },
}));
