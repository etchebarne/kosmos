import type { SettingsSnapshot } from "../shared/ipc";
import { validateActionResult } from "../shared/ipc/generated/validators";

export function isSettingsSnapshot(
  action: "get" | "update",
  value: unknown,
): value is SettingsSnapshot {
  return validateActionResult("settings", action, value);
}

export async function loadBootstrapSettings(
  request: () => Promise<unknown>,
): Promise<SettingsSnapshot> {
  const snapshot = await request();
  if (!isSettingsSnapshot("get", snapshot)) {
    throw new Error("Invalid settings.get result from server during startup");
  }

  return snapshot;
}

export function newerSettingsSnapshot(
  current: SettingsSnapshot | undefined,
  next: SettingsSnapshot,
): SettingsSnapshot | undefined {
  return !current || next.revision > current.revision ? next : undefined;
}
