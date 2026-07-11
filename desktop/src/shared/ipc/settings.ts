export const APPEARANCE_ZOOM_LEVEL = "appearance.zoomLevel";

export type SettingValue = boolean | string | number;

export type SettingControl =
  | { type: "switch" }
  | { type: "select"; options: SettingOption[] }
  | {
      type: "input";
      inputType: "text" | "number";
      placeholder?: string | null;
      min?: number | null;
      max?: number | null;
      step?: number | null;
    };

export type SettingOption = {
  value: string;
  label: string;
};

export type SettingItem = SettingGroup | SettingDefinition;

export type SettingGroup = {
  type: "group";
  id: string;
  label: string;
  description?: string | null;
  items: SettingItem[];
};

export type SettingDefinition = {
  type: "setting";
  id: string;
  label: string;
  description?: string | null;
  control: SettingControl;
  value: SettingValue;
  defaultValue: SettingValue;
};

export type SettingCategory = {
  id: string;
  label: string;
  description?: string | null;
  items: SettingItem[];
};

export type SettingsSnapshot = {
  categories: SettingCategory[];
};

export type UpdateSettingParams = {
  id: string;
  value: SettingValue;
};
