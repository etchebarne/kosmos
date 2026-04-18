import type { TabDefinition } from "./types";

const registry = new Map<string, TabDefinition>();

export function registerTab(definition: TabDefinition) {
  registry.set(definition.type, definition);
}

export function getTabDefinition(type: string): TabDefinition | undefined {
  return registry.get(type);
}

export function unregisterTab(type: string) {
  registry.delete(type);
}

export function getVisibleTabDefinitions(): TabDefinition[] {
  return Array.from(registry.values()).filter((d) => !d.hidden);
}
