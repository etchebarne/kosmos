import { describe, expect, test } from "bun:test";

import {
  createWindowZoomPolicyCache,
  handleWindowZoom,
  updateWindowZoomPolicyCache,
} from "@/main/window/shortcuts";
import type { SettingsSnapshot } from "@/shared/ipc";

describe("window zoom shortcuts", () => {
  test("uses only the newest server policy for clamping and shortcuts", () => {
    const policy = createWindowZoomPolicyCache(settingsSnapshot(4));
    expect(updateWindowZoomPolicyCache(policy, settingsSnapshot(3))).toBe(false);
    expect(updateWindowZoomPolicyCache(policy, settingsSnapshot(5, 105, 70, 150, 5))).toBe(true);

    const window = fakeWindow(148);
    expect(handleWindowZoom(window, policy, "in")).toBe(150);
    expect(handleWindowZoom(window, policy, "out")).toBe(145);
    expect(handleWindowZoom(window, policy, "reset")).toBe(105);
    expect(window.zoomLevels).toEqual([150, 145, 105]);
  });
});

function settingsSnapshot(
  revision: number,
  defaultZoomLevel = 100,
  minZoomLevel = 80,
  maxZoomLevel = 140,
  zoomStep = 10,
): SettingsSnapshot {
  return {
    revision,
    editor: { softWrap: false, minimap: false, formatOnSave: false },
    appearance: {
      zoomSettingId: "zoom-setting",
      zoomLevel: defaultZoomLevel,
      defaultZoomLevel,
      minZoomLevel,
      maxZoomLevel,
      zoomStep,
    },
    categories: [],
  };
}

function fakeWindow(initialZoomLevel: number) {
  const zoomLevels: number[] = [];
  let zoomFactor = initialZoomLevel / 100;
  return {
    zoomLevels,
    webContents: {
      getZoomFactor: () => zoomFactor,
      setZoomFactor: (nextZoomFactor: number) => {
        zoomFactor = nextZoomFactor;
        zoomLevels.push(nextZoomFactor * 100);
      },
    },
  } as unknown as Electron.BrowserWindow & { zoomLevels: number[] };
}
