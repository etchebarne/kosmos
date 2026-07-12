export function formattingErrorAfterContextChange(
  error: string | null,
  options: { formattingEnabled: boolean; documentChanged: boolean },
): string | null {
  return options.formattingEnabled && !options.documentChanged ? error : null;
}
