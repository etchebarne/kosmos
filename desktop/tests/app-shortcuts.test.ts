import { describe, expect, test } from "bun:test";

import { appShortcutForEvent, isTerminalOwnedShortcut } from "@/renderer/lib/app-shortcuts";

describe("app shortcuts", () => {
  test("a focused terminal keeps Ctrl+W for the shell but not the other shortcuts", () => {
    expect(isTerminalOwnedShortcut("closeTab")).toBe(true);
    expect(isTerminalOwnedShortcut("quickOpen")).toBe(false);
    expect(isTerminalOwnedShortcut("nextTab")).toBe(false);
  });

  test("matches Ctrl or Cmd letter shortcuts", () => {
    expect(appShortcutForEvent(keyboardEvent({ key: "w" }))).toBe("closeTab");
    expect(appShortcutForEvent(keyboardEvent({ key: "p" }))).toBe("quickOpen");
    expect(appShortcutForEvent(keyboardEvent({ key: "t" }))).toBe("workspaceSymbols");
    expect(appShortcutForEvent(keyboardEvent({ key: "W" }))).toBe("closeTab");
    expect(appShortcutForEvent(keyboardEvent({ ctrlKey: false, metaKey: true, key: "p" }))).toBe(
      "quickOpen",
    );
  });

  test("cycles tabs with Ctrl+Tab and Ctrl+Shift+Tab", () => {
    expect(appShortcutForEvent(keyboardEvent({ key: "Tab" }))).toBe("nextTab");
    expect(appShortcutForEvent(keyboardEvent({ key: "Tab", shiftKey: true }))).toBe("previousTab");
  });

  test("ignores combinations with extra or missing modifiers", () => {
    expect(appShortcutForEvent(keyboardEvent({ key: "t", shiftKey: true }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "w", shiftKey: true }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "p", altKey: true }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "p", metaKey: true }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "w", ctrlKey: false }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "Tab", ctrlKey: false }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "Tab", altKey: true }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "Tab", metaKey: true }))).toBeNull();
  });

  test("ignores unrelated keys", () => {
    expect(appShortcutForEvent(keyboardEvent({ key: "s" }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "c" }))).toBeNull();
    expect(appShortcutForEvent(keyboardEvent({ key: "Enter" }))).toBeNull();
  });
});

function keyboardEvent(overrides: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    altKey: false,
    ctrlKey: true,
    key: "w",
    metaKey: false,
    shiftKey: false,
    ...overrides,
  } as KeyboardEvent;
}
