import { create } from "zustand";

import {
  getLanguageServerStatus,
  installLanguageServer as installLanguageServerIpc,
  listLanguageServers,
  restartLanguageServer as restartLanguageServerIpc,
  uninstallLanguageServer as uninstallLanguageServerIpc,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import {
  languageServerOperationInProgress,
  pendingServersAfterStatus,
  statusRetryDelay,
} from "@/renderer/lib/language-server-state";
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

export const useLanguageServerStore = create<LanguageServerStore>((set, get) => {
  let initialization: Promise<void> | null = null;
  let pushHandlingStarted = false;
  const statusRefreshes = new Map<string, Promise<void>>();
  const repeatedStatusRefreshes = new Set<string>();
  const statusRetryAttempts = new Map<string, number>();
  const statusRetryTimers = new Map<string, number>();

  function scheduleStatusRefresh(serverId: string): void {
    if (statusRetryTimers.has(serverId)) {
      return;
    }
    const attempt = statusRetryAttempts.get(serverId) ?? 0;
    statusRetryAttempts.set(serverId, attempt + 1);
    const timer = window.setTimeout(() => {
      statusRetryTimers.delete(serverId);
      void refreshStatus(serverId);
    }, statusRetryDelay(attempt));
    statusRetryTimers.set(serverId, timer);
  }

  function clearStatusRetry(serverId: string): void {
    const timer = statusRetryTimers.get(serverId);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      statusRetryTimers.delete(serverId);
    }
    statusRetryAttempts.delete(serverId);
  }

  function startPushHandling(): void {
    if (pushHandlingStarted) {
      return;
    }
    pushHandlingStarted = true;
    window.kosmos.onServerNotification((notification) => {
      if (
        notification.event === "languageServerStatusChanged" ||
        notification.event === "languageServerLogAvailable"
      ) {
        void refreshStatus(notification.serverId);
      }
    });
    window.kosmos.onServerReconnected(() => {
      void get().initializeLanguageServers();
    });
  }

  function refreshStatus(serverId: string): Promise<void> {
    const active = statusRefreshes.get(serverId);
    if (active) {
      repeatedStatusRefreshes.add(serverId);
      return active;
    }
    const refresh = getLanguageServerStatus({ serverId })
      .then((status) => {
        clearStatusRetry(serverId);
        set((state) => ({
          error: null,
          servers: replaceServer(state.servers, status),
          pendingServerIds: pendingServersAfterStatus(state.pendingServerIds, status),
        }));
        if (languageServerOperationInProgress(status)) {
          scheduleStatusRefresh(serverId);
        }
      })
      .catch((caughtError: unknown) => {
        set((state) => ({
          error: errorMessage(caughtError),
          pendingServerIds: withoutKey(state.pendingServerIds, serverId),
        }));
        scheduleStatusRefresh(serverId);
      })
      .finally(() => {
        statusRefreshes.delete(serverId);
        if (repeatedStatusRefreshes.delete(serverId)) {
          void refreshStatus(serverId);
        }
      });
    statusRefreshes.set(serverId, refresh);
    return refresh;
  }

  async function initialize(): Promise<void> {
    startPushHandling();
    set({ error: null, isLoading: true });
    try {
      const snapshot = await listLanguageServers();
      set({ servers: snapshot.servers });
      snapshot.servers.forEach((server) => {
        clearStatusRetry(server.id);
      });
      set({
        pendingServerIds: Object.fromEntries(
          snapshot.servers
            .filter(languageServerOperationInProgress)
            .map((server) => [server.id, true]),
        ),
      });
      snapshot.servers
        .filter(languageServerOperationInProgress)
        .forEach((server) => scheduleStatusRefresh(server.id));
    } catch (caughtError) {
      set({ error: errorMessage(caughtError), pendingServerIds: {} });
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
        set((state) => ({
          pendingServerIds: pendingServersAfterStatus(state.pendingServerIds, status),
        }));
        if (languageServerOperationInProgress(status)) {
          scheduleStatusRefresh(serverId);
        }
      })
      .catch((caughtError: unknown) => {
        set((state) => ({
          error: errorMessage(caughtError),
          pendingServerIds: withoutKey(state.pendingServerIds, serverId),
        }));
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
