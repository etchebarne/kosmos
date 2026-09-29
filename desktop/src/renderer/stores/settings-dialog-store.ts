import { create } from "zustand";

export const LANGUAGE_SERVERS_SETTINGS_SECTION = "languageServers";

type SettingsDialogStore = {
  open: boolean;
  sectionId: string | null;
  openSettings(sectionId?: string): void;
  setSettingsOpen(open: boolean): void;
  selectSettingsSection(sectionId: string): void;
};

export const useSettingsDialogStore = create<SettingsDialogStore>((set) => ({
  open: false,
  sectionId: null,
  openSettings(sectionId) {
    set((state) => ({ open: true, sectionId: sectionId ?? state.sectionId }));
  },
  setSettingsOpen(open) {
    set({ open });
  },
  selectSettingsSection(sectionId) {
    set({ sectionId });
  },
}));
