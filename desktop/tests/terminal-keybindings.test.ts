import { describe, expect, test } from "bun:test";

import { terminalCopyText } from "@/renderer/lib/terminal-keybindings";

describe("terminal copy shortcut", () => {
  test("copies selected text with Ctrl+C or Cmd+C", () => {
    expect(terminalCopyText(keyboardEvent({ ctrlKey: true }), "selected text")).toBe(
      "selected text",
    );
    expect(terminalCopyText(keyboardEvent({ metaKey: true }), "selected text")).toBe(
      "selected text",
    );
  });

  test("leaves Ctrl+C to the shell when there is no selection", () => {
    expect(terminalCopyText(keyboardEvent({ ctrlKey: true }), "")).toBeNull();
  });

  test("ignores unrelated key events", () => {
    expect(terminalCopyText(keyboardEvent({ ctrlKey: true, key: "v" }), "selected text")).toBeNull();
    expect(
      terminalCopyText(keyboardEvent({ ctrlKey: true, altKey: true }), "selected text"),
    ).toBeNull();
    expect(
      terminalCopyText(keyboardEvent({ ctrlKey: true, type: "keyup" }), "selected text"),
    ).toBeNull();
  });
});

function keyboardEvent(overrides: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    altKey: false,
    ctrlKey: false,
    key: "c",
    metaKey: false,
    type: "keydown",
    ...overrides,
  } as KeyboardEvent;
}
