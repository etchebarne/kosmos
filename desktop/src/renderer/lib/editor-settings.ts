import type { SettingsSnapshot } from "@/shared/ipc";

export type EditorSettings = {
  softWrap: boolean;
  minimap: boolean;
  formatOnSave: boolean;
};

export function editorSettings(snapshot: SettingsSnapshot | null): EditorSettings | null {
  return snapshot?.editor ?? null;
}
