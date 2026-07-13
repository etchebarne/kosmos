import type { SettingsSnapshot, UpdateSettingParams } from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "settings";

export function getSettings(): Promise<SettingsSnapshot> {
  return requestServer(DOMAIN, "get");
}

export function updateSetting(params: UpdateSettingParams): Promise<SettingsSnapshot> {
  return requestServer(DOMAIN, "update", params);
}
