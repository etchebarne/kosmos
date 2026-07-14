type TerminalKeyboardEvent = Pick<
  KeyboardEvent,
  "altKey" | "ctrlKey" | "key" | "metaKey" | "type"
>;

export function terminalCopyText(event: TerminalKeyboardEvent, selection: string): string | null {
  if (
    event.type !== "keydown" ||
    event.altKey ||
    (!event.ctrlKey && !event.metaKey) ||
    event.key.toLowerCase() !== "c" ||
    selection.length === 0
  ) {
    return null;
  }

  return selection;
}
