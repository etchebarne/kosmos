export type AppShortcut = "closeTab" | "nextTab" | "previousTab" | "quickOpen" | "workspaceSymbols";

type ShortcutKeyboardEvent = Pick<KeyboardEvent, "altKey" | "ctrlKey" | "key" | "metaKey" | "shiftKey">;

const LETTER_SHORTCUTS: Readonly<Record<string, AppShortcut>> = {
  p: "quickOpen",
  t: "workspaceSymbols",
  w: "closeTab",
};

/** Shortcuts a focused terminal keeps for the shell; Ctrl+W deletes the previous word. */
const TERMINAL_OWNED_SHORTCUTS: ReadonlySet<AppShortcut> = new Set(["closeTab"]);

export function isTerminalOwnedShortcut(shortcut: AppShortcut): boolean {
  return TERMINAL_OWNED_SHORTCUTS.has(shortcut);
}

/** Maps a key event to the app-level shortcut it triggers, if any. */
export function appShortcutForEvent(event: ShortcutKeyboardEvent): AppShortcut | null {
  if (event.altKey) {
    return null;
  }

  if (event.key === "Tab") {
    return tabCycleShortcut(event);
  }

  return letterShortcut(event);
}

function tabCycleShortcut(event: ShortcutKeyboardEvent): AppShortcut | null {
  if (!event.ctrlKey || event.metaKey) {
    return null;
  }

  return event.shiftKey ? "previousTab" : "nextTab";
}

function letterShortcut(event: ShortcutKeyboardEvent): AppShortcut | null {
  if (!hasSinglePrimaryModifier(event) || event.shiftKey) {
    return null;
  }

  return LETTER_SHORTCUTS[event.key.toLowerCase()] ?? null;
}

function hasSinglePrimaryModifier(event: ShortcutKeyboardEvent): boolean {
  return event.ctrlKey !== event.metaKey;
}
