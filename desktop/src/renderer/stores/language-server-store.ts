import { create } from "zustand";

import {
  getLanguageServerStatus,
  installLanguageServer as installLanguageServerIpc,
  listLanguageServers,
  restartLanguageServer as restartLanguageServerIpc,
  uninstallLanguageServer as uninstallLanguageServerIpc,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import { applyLanguageServerStatus } from "@/renderer/lib/language-client";
import type { LanguageServerSnapshot } from "@/shared/ipc";

type LanguageServerStore = {
  servers: LanguageServerSnapshot[];
  error: string | null;
  isLoading: boolean;
  pendingServerIds: Record<string, true>;
  initializeLanguageServers(): Promise<void>;
  installLanguageServer(serverId: string): void;
  uninstallLanguageServer(serverId: string): void;
  restartLanguageServer(serverId: string): void;
};

const POLL_INTERVAL_MS = 500;
const RUNTIME_POLL_INTERVAL_MS = 2_000;

export const useLanguageServerStore = create<LanguageServerStore>((set, get) => {
  let initialization: Promise<void> | null = null;
  let runtimePollingStarted = false;

  function startRuntimePolling(): void {
    if (runtimePollingStarted) {
      return;
    }
    runtimePollingStarted = true;
    window.setInterval(() => {
      void listLanguageServers()
        .then((snapshot) => set({ servers: snapshot.servers }))
        .catch(() => {
          // Action errors remain user-visible; background status refreshes are best effort.
        });
    }, RUNTIME_POLL_INTERVAL_MS);
  }

  function resumePolling(serverId: string): void {
    if (get().pendingServerIds[serverId]) {
      return;
    }
    set((state) => ({
      pendingServerIds: { ...state.pendingServerIds, [serverId]: true },
    }));
    void pollUntilSettled(serverId, set)
      .catch((caughtError: unknown) => {
        set({ error: errorMessage(caughtError) });
      })
      .finally(() => {
        set((state) => ({ pendingServerIds: withoutKey(state.pendingServerIds, serverId) }));
      });
  }

  async function initialize(): Promise<void> {
    set({ error: null, isLoading: true });
    try {
      const snapshot = await listLanguageServers();
      set({ servers: snapshot.servers });
      for (const server of snapshot.servers) {
        if (isInProgress(server)) {
          resumePolling(server.id);
        }
      }
      startRuntimePolling();
    } catch (caughtError) {
      set({ error: errorMessage(caughtError) });
    } finally {
      set({ isLoading: false });
      initialization = null;
    }
  }

  function runAction(
    serverId: string,
    action: (params: { serverId: string }) => Promise<LanguageServerSnapshot>,
  ): void {
    if (get().pendingServerIds[serverId]) {
      return;
    }

    set((state) => ({
      error: null,
      pendingServerIds: { ...state.pendingServerIds, [serverId]: true },
    }));

    void action({ serverId })
      .then((status) => {
        set((state) => ({ servers: replaceServer(state.servers, status) }));
        return pollUntilSettled(serverId, set);
      })
      .catch((caughtError: unknown) => {
        set({ error: errorMessage(caughtError) });
      })
      .finally(() => {
        set((state) => ({ pendingServerIds: withoutKey(state.pendingServerIds, serverId) }));
      });
  }

  return {
    servers: [],
    error: null,
    isLoading: false,
    pendingServerIds: {},
    initializeLanguageServers() {
      initialization ??= initialize();
      return initialization;
    },
    installLanguageServer(serverId) {
      runAction(serverId, installLanguageServerIpc);
    },
    uninstallLanguageServer(serverId) {
      runAction(serverId, uninstallLanguageServerIpc);
    },
    restartLanguageServer(serverId) {
      runAction(serverId, restartLanguageServerIpc);
    },
  };
});

async function pollUntilSettled(
  serverId: string,
  set: (
    partial:
      | Partial<LanguageServerStore>
      | ((state: LanguageServerStore) => Partial<LanguageServerStore>),
  ) => void,
): Promise<void> {
  while (true) {
    await delay(POLL_INTERVAL_MS);
    const status = await getLanguageServerStatus({ serverId });
    set((state) => ({ servers: replaceServer(state.servers, status) }));
    if (!isInProgress(status)) {
      applyLanguageServerStatus(status);
      return;
    }
  }
}

function isInProgress(server: LanguageServerSnapshot): boolean {
  return server.installationState === "installing" || server.installationState === "uninstalling";
}

function replaceServer(
  servers: LanguageServerSnapshot[],
  replacement: LanguageServerSnapshot,
): LanguageServerSnapshot[] {
  return servers.map((server) => (server.id === replacement.id ? replacement : server));
}

function withoutKey(values: Record<string, true>, key: string): Record<string, true> {
  const nextValues = { ...values };
  delete nextValues[key];
  return nextValues;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}
