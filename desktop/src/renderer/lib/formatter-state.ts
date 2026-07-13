import type { FormatterSnapshot } from "@/shared/ipc";

export function reorderFormatters(
  formatters: readonly FormatterSnapshot[],
  formatterIds: readonly string[],
): FormatterSnapshot[] {
  const byId = new Map(formatters.map((formatter) => [formatter.id, formatter]));
  return formatterIds.flatMap((id, priority) => {
    const formatter = byId.get(id);
    return formatter ? [{ ...formatter, priority }] : [];
  });
}

export function restoreFormatterPriorityOrder(
  current: readonly FormatterSnapshot[],
  previousIds: readonly string[],
): FormatterSnapshot[] {
  return reorderFormatters(current, previousIds);
}
