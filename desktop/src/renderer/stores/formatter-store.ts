import { create } from "zustand";

import {
  getFormatterStatus,
  installFormatter as installFormatterIpc,
  listFormatters,
  uninstallFormatter as uninstallFormatterIpc,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import type { FormatterSnapshot } from "@/shared/ipc";

type FormatterStore = {
  formatters: FormatterSnapshot[];
  error: string | null;
  isLoading: boolean;
  pendingFormatterIds: Record<string, true>;
  initializeFormatters(): Promise<void>;
  installFormatter(formatterId: string): void;
  uninstallFormatter(formatterId: string): void;
};

const POLL_INTERVAL_MS = 500;

export const useFormatterStore = create<FormatterStore>((set, get) => {
  let initialization: Promise<void> | null = null;

  async function initialize(): Promise<void> {
    set({ error: null, isLoading: true });
    try {
      const snapshot = await listFormatters();
      set({ formatters: snapshot.formatters });
      for (const formatter of snapshot.formatters) {
        if (isInProgress(formatter)) runPolling(formatter.id);
      }
    } catch (caughtError) {
      set({ error: errorMessage(caughtError) });
    } finally {
      set({ isLoading: false });
      initialization = null;
    }
  }

  function runPolling(formatterId: string): void {
    if (get().pendingFormatterIds[formatterId]) return;
    set((state) => ({
      pendingFormatterIds: { ...state.pendingFormatterIds, [formatterId]: true },
    }));
    void pollUntilSettled(formatterId, set)
      .catch((caughtError: unknown) => set({ error: errorMessage(caughtError) }))
      .finally(() => {
        set((state) => ({
          pendingFormatterIds: withoutKey(state.pendingFormatterIds, formatterId),
        }));
      });
  }

  function runAction(
    formatterId: string,
    action: (params: { formatterId: string }) => Promise<FormatterSnapshot>,
  ): void {
    if (get().pendingFormatterIds[formatterId]) return;
    set((state) => ({
      error: null,
      pendingFormatterIds: { ...state.pendingFormatterIds, [formatterId]: true },
    }));
    void action({ formatterId })
      .then((status) => {
        set((state) => ({ formatters: replaceFormatter(state.formatters, status) }));
        return pollUntilSettled(formatterId, set);
      })
      .catch((caughtError: unknown) => set({ error: errorMessage(caughtError) }))
      .finally(() => {
        set((state) => ({
          pendingFormatterIds: withoutKey(state.pendingFormatterIds, formatterId),
        }));
      });
  }

  return {
    formatters: [],
    error: null,
    isLoading: false,
    pendingFormatterIds: {},
    initializeFormatters() {
      initialization ??= initialize();
      return initialization;
    },
    installFormatter(formatterId) {
      runAction(formatterId, installFormatterIpc);
    },
    uninstallFormatter(formatterId) {
      runAction(formatterId, uninstallFormatterIpc);
    },
  };
});

async function pollUntilSettled(
  formatterId: string,
  set: (
    partial:
      | Partial<FormatterStore>
      | ((state: FormatterStore) => Partial<FormatterStore>),
  ) => void,
): Promise<void> {
  while (true) {
    await new Promise((resolve) => window.setTimeout(resolve, POLL_INTERVAL_MS));
    const status = await getFormatterStatus({ formatterId });
    set((state) => ({ formatters: replaceFormatter(state.formatters, status) }));
    if (!isInProgress(status)) return;
  }
}

function isInProgress(formatter: FormatterSnapshot): boolean {
  return formatter.installationState === "installing" || formatter.installationState === "uninstalling";
}

function replaceFormatter(formatters: FormatterSnapshot[], replacement: FormatterSnapshot) {
  return formatters.map((formatter) => formatter.id === replacement.id ? replacement : formatter);
}

function withoutKey(values: Record<string, true>, key: string): Record<string, true> {
  const next = { ...values };
  delete next[key];
  return next;
}
