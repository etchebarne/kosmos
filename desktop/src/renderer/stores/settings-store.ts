import { create } from "zustand";

import { getSettings, updateSetting as updateSettingIpc } from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import type {
  SettingDefinition,
  SettingItem,
  SettingsSnapshot,
  SettingValue,
} from "@/shared/ipc";

type SettingsStore = {
  error: string | null;
  isLoading: boolean;
  pendingSettingIds: Record<string, true>;
  snapshot: SettingsSnapshot | null;
  initializeSettings(): Promise<void>;
  updateSetting(id: string, value: SettingValue): void;
};

type PendingUpdate = {
  requestId: number;
  value: SettingValue;
};

export const useSettingsStore = create<SettingsStore>((set, get) => {
  const pendingUpdates = new Map<string, PendingUpdate>();
  let nextRequestId = 0;

  return {
    error: null,
    isLoading: true,
    pendingSettingIds: {},
    snapshot: null,
    async initializeSettings() {
      set({ error: null, isLoading: true });

      try {
        const snapshot = await getSettings();
        set({ snapshot: applyPendingUpdates(snapshot, pendingUpdates) });
      } catch (caughtError) {
        set({ error: errorMessage(caughtError) });
      } finally {
        set({ isLoading: false });
      }
    },
    updateSetting(id, value) {
      const snapshot = get().snapshot;
      const previous = findSetting(snapshot, id)?.value;
      if (!snapshot || previous === undefined || previous === value) {
        return;
      }

      const requestId = ++nextRequestId;
      pendingUpdates.set(id, { requestId, value });
      set((state) => ({
        error: null,
        pendingSettingIds: { ...state.pendingSettingIds, [id]: true },
        snapshot: updateSnapshotValue(state.snapshot, id, value),
      }));

      void updateSettingIpc({ id, value })
        .then((serverSnapshot) => {
          if (pendingUpdates.get(id)?.requestId !== requestId) {
            return;
          }

          pendingUpdates.delete(id);
          set((state) => ({
            pendingSettingIds: withoutKey(state.pendingSettingIds, id),
            snapshot: applyPendingUpdates(serverSnapshot, pendingUpdates),
          }));
        })
        .catch((caughtError: unknown) => {
          if (pendingUpdates.get(id)?.requestId !== requestId) {
            return;
          }

          pendingUpdates.delete(id);
          set((state) => ({
            error: errorMessage(caughtError),
            pendingSettingIds: withoutKey(state.pendingSettingIds, id),
            snapshot: updateSnapshotValue(state.snapshot, id, previous),
          }));
        });
    },
  };
});

export function findSetting(
  snapshot: SettingsSnapshot | null,
  id: string,
): SettingDefinition | undefined {
  for (const category of snapshot?.categories ?? []) {
    const setting = findSettingInItems(category.items, id);
    if (setting) {
      return setting;
    }
  }

  return undefined;
}

function findSettingInItems(items: SettingItem[], id: string): SettingDefinition | undefined {
  for (const item of items) {
    if (item.type === "setting" && item.id === id) {
      return item;
    }

    if (item.type === "group") {
      const setting = findSettingInItems(item.items, id);
      if (setting) {
        return setting;
      }
    }
  }

  return undefined;
}

function updateSnapshotValue(
  snapshot: SettingsSnapshot | null,
  id: string,
  value: SettingValue,
): SettingsSnapshot | null {
  if (!snapshot) {
    return null;
  }

  return {
    categories: snapshot.categories.map((category) => ({
      ...category,
      items: updateItemsValue(category.items, id, value),
    })),
  };
}

function updateItemsValue(items: SettingItem[], id: string, value: SettingValue): SettingItem[] {
  return items.map((item) => {
    if (item.type === "setting") {
      return item.id === id ? { ...item, value } : item;
    }

    return { ...item, items: updateItemsValue(item.items, id, value) };
  });
}

function applyPendingUpdates(
  snapshot: SettingsSnapshot,
  pendingUpdates: Map<string, PendingUpdate>,
): SettingsSnapshot {
  let nextSnapshot: SettingsSnapshot | null = snapshot;

  for (const [id, update] of pendingUpdates) {
    nextSnapshot = updateSnapshotValue(nextSnapshot, id, update.value);
  }

  return nextSnapshot ?? snapshot;
}

function withoutKey(values: Record<string, true>, key: string): Record<string, true> {
  const nextValues = { ...values };
  delete nextValues[key];
  return nextValues;
}
