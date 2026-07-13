import { describe, expect, test } from "bun:test";

import { editorSettings } from "@/renderer/lib/editor-settings";
import { applySettingsSnapshot } from "@/renderer/stores/settings-store";
import type { SettingsSnapshot } from "@/shared/ipc";

describe("settings projections", () => {
  test("uses the resolved editor behavior from the default snapshot", () => {
    expect(editorSettings(settingsSnapshot())).toEqual({
      softWrap: false,
      minimap: false,
      formatOnSave: false,
    });
  });

  test("uses updated resolved behavior without catalog ID lookups", () => {
    const snapshot = settingsSnapshot(5, {
      softWrap: true,
      minimap: true,
      formatOnSave: true,
    });

    expect(editorSettings(snapshot)).toEqual(snapshot.editor);
  });

  test("ignores settings snapshots older than the renderer cache", () => {
    const current = settingsSnapshot(5, {
      softWrap: true,
      minimap: false,
      formatOnSave: true,
    });
    const stale = settingsSnapshot(3);

    expect(applySettingsSnapshot(current, stale)).toBe(current);
  });
});

function settingsSnapshot(
  revision = 0,
  editor = { softWrap: false, minimap: false, formatOnSave: false },
): SettingsSnapshot {
  return {
    revision,
    editor,
    appearance: {
      zoomSettingId: "zoom-setting",
      zoomLevel: 100,
      defaultZoomLevel: 100,
      minZoomLevel: 80,
      maxZoomLevel: 140,
      zoomStep: 10,
    },
    categories: [],
  };
}
