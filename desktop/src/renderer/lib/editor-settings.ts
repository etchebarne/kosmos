import type { SettingsSnapshot } from "@/shared/ipc";

import { findSetting } from "@/renderer/stores/settings-store";

const SOFT_WRAP_SETTING_ID = "editor.softWrap";
const MINIMAP_SETTING_ID = "editor.minimap";

export type EditorSettings = {
  softWrap: boolean;
  minimap: boolean;
};

export function editorSettings(snapshot: SettingsSnapshot | null): EditorSettings {
  return {
    softWrap: booleanSetting(snapshot, SOFT_WRAP_SETTING_ID),
    minimap: booleanSetting(snapshot, MINIMAP_SETTING_ID),
  };
}

function booleanSetting(snapshot: SettingsSnapshot | null, id: string): boolean {
  const value = findSetting(snapshot, id)?.value;
  return typeof value === "boolean" ? value : false;
}
