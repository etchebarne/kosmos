import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import { invoke } from "@tauri-apps/api/core";
import type { Monaco } from "@monaco-editor/react";
import { useToastStore } from "./toast.store";
import type { LspClient } from "../lib/lsp/client";
import type { ServerStatus, LspState, ServerAvailability } from "./lsp.types";
import {
  SHUTDOWN_TIMEOUT_MS,
  cleanupProgressForWorkspace,
  cleanupRestartStateForWorkspace,
} from "./lsp.lifecycle";
import {
  ensureLanguageGroups,
  resolveServerLanguage,
  getCompanionsFor,
  pending,
  setServerInfo,
  ensureProviders,
  initializeServer,
} from "./lsp.init";

export type { ServerStatus } from "./lsp.types";
export { resolveServerLanguage } from "./lsp.init";

// Kept so post-install restarts can re-register providers without a new mount.
let monacoRef: Monaco | null = null;

export const useLspStore = create<LspState>()(
  immer((set, get) => {
    return {
      servers: {},
      availability: {},
      indexProgress: {},

      checkAvailability: async (workspacePath) => {
        try {
          const result = await invoke<ServerAvailability[]>("lsp_check_availability", {
            workspacePath,
          });
          set((state) => {
            state.availability[workspacePath] = result;
          });
        } catch (err) {
          console.error("Failed to check LSP availability:", err);
        }
      },

      warmupWorkspace: async (workspacePath) => {
        await ensureLanguageGroups();

        // Deep-scan for project markers at any depth so each server gets its own root.
        let projects: { languageId: string; projectRoot: string; available: boolean }[];
        try {
          projects = await invoke<typeof projects>("lsp_scan_projects", { workspacePath });
        } catch {
          return;
        }

        for (const project of projects) {
          if (!project.available) continue;

          const serverLang = resolveServerLanguage(project.languageId);
          const existing = get().servers[workspacePath]?.[serverLang];
          if (existing) continue;

          initializeServer(
            workspacePath,
            project.projectRoot,
            project.languageId,
            monacoRef,
            set,
            get,
          ).catch((err) => {
            console.warn(`[kosmos:lsp] Warmup failed for ${project.languageId}:`, err);
          });
        }
      },

      startServer: async (workspacePath, languageId, filePath, monaco) => {
        monacoRef = monaco;
        await ensureLanguageGroups();
        const serverLang = resolveServerLanguage(languageId);

        // If the server was warmed up, providers still need registering with this Monaco.
        const existing = get().servers[workspacePath]?.[serverLang];
        if (existing && existing.status === "running") {
          ensureProviders(workspacePath, serverLang, existing, monaco, set);
          return existing.client;
        }

        if (existing?.status === "unavailable" || existing?.status === "installing") {
          return null;
        }

        // Walk up from the file to the nearest project marker for the rootUri.
        let projectRoot = workspacePath;
        if (filePath) {
          try {
            projectRoot = await invoke<string>("lsp_resolve_root", {
              filePath,
              languageId,
              workspacePath,
            });
          } catch {
            // Fall back to workspace root.
          }
        }

        try {
          const pendingKey = `${workspacePath}:${serverLang}`;
          const inflight = pending.get(pendingKey);

          const client = inflight
            ? await inflight
            : await initializeServer(workspacePath, projectRoot, languageId, monacoRef, set, get);
          if (client) {
            const info = get().servers[workspacePath]?.[serverLang];
            if (info) ensureProviders(workspacePath, serverLang, info, monaco, set);
          }
          return client;
        } catch (err) {
          const errorStr = String(err);

          if (errorStr.includes("No language server configured")) {
            return null;
          }

          const isNotFound =
            /not found|No such file|program not found|os error 2|cannot find|Unknown binary/i.test(
              errorStr,
            );

          // Binary exists but crashed during init (e.g. rustup shim w/o component).
          const isStartupCrash = !isNotFound && errorStr.includes("Language server stopped");

          const nameMatch =
            errorStr.match(/Failed to start ([^:]+):/) ??
            errorStr.match(/Unknown binary '([^']+)'/);
          const displayName = nameMatch?.[1] ?? serverLang;

          const canInstall = isNotFound || isStartupCrash;
          const status: ServerStatus = canInstall ? "unavailable" : "error";
          const errorMessage = isNotFound
            ? `${displayName} is not installed`
            : `${displayName} failed to start`;

          console.error(`LSP ${status} for ${serverLang}:`, err);

          // Race: another startServer may have already set a terminal state.
          const already = get().servers[workspacePath]?.[serverLang];
          if (already?.status === "unavailable" || already?.status === "error") {
            return null;
          }

          setServerInfo(
            workspacePath,
            serverLang,
            {
              serverId: "",
              languageId: serverLang,
              client: null,
              status,
              serverName: displayName,
              errorMessage,
              providerDisposables: [],
            },
            set,
          );

          if (canInstall) {
            const { installServer } = get();
            useToastStore.getState().addToast({
              message: errorMessage,
              type: "warning",
              action: {
                label: "Install",
                onClick: () => installServer(workspacePath, displayName),
              },
            });
          }

          return null;
        }
      },

      getClient: (workspacePath, languageId) => {
        const serverLang = resolveServerLanguage(languageId);
        const info = get().servers[workspacePath]?.[serverLang];
        return info?.status === "running" ? info.client : null;
      },

      getCompanionClients: (workspacePath, languageId) => {
        const serverLang = resolveServerLanguage(languageId);
        const companions = getCompanionsFor(serverLang);
        const clients: LspClient[] = [];
        for (const companionLang of companions) {
          const info = get().servers[workspacePath]?.[companionLang];
          if (info?.status === "running" && info.client) {
            clients.push(info.client);
          }
        }
        return clients;
      },

      startCompanions: async (workspacePath, primaryServerLang, filePath, monaco) => {
        const serverLang = resolveServerLanguage(primaryServerLang);
        const companions = getCompanionsFor(serverLang);
        for (const companionLang of companions) {
          const existing = get().servers[workspacePath]?.[companionLang];
          if (existing) continue;

          get()
            .startServer(workspacePath, companionLang, filePath, monaco)
            .catch((err) => {
              console.warn(`[kosmos:lsp] Companion ${companionLang} failed:`, err);
            });
        }
      },

      installServer: async (workspacePath, serverName) => {
        const workspace = get().servers[workspacePath];
        const serverLang = workspace
          ? Object.keys(workspace).find((lang) => workspace[lang].serverName === serverName)
          : undefined;

        if (serverLang) {
          set((state) => {
            const server = state.servers[workspacePath]?.[serverLang];
            if (server) {
              server.status = "installing";
              server.errorMessage = null;
            }
          });
        }

        try {
          await invoke("lsp_install_server", { name: serverName, workspacePath });

          useToastStore.getState().addToast({
            message: `${serverName} installed successfully`,
            type: "success",
          });

          if (serverLang) {
            set((state) => {
              const server = state.servers[workspacePath]?.[serverLang];
              if (server) {
                server.status = "stopped";
                server.errorMessage = null;
              }
            });
          }

          if (monacoRef && serverLang) {
            await get().startServer(workspacePath, serverLang, null, monacoRef);
          }
        } catch (err) {
          const errorMessage = `Failed to install ${serverName}: ${err}`;
          console.error(errorMessage);

          useToastStore.getState().addToast({
            message: errorMessage,
            type: "error",
            duration: 12000,
          });

          if (serverLang) {
            set((state) => {
              const server = state.servers[workspacePath]?.[serverLang];
              if (server) {
                server.status = "unavailable";
                server.errorMessage = errorMessage;
              }
            });
          }
        }
      },

      stopWorkspace: async (workspacePath) => {
        const workspace = get().servers[workspacePath];
        if (!workspace) return;

        const shutdownPromises = Object.values(workspace).map(async (info) => {
          for (const d of info.providerDisposables) {
            d.dispose();
          }
          if (info.client && info.status === "running") {
            try {
              await Promise.race([
                info.client.shutdown(),
                new Promise<void>((_, reject) =>
                  setTimeout(() => reject(new Error("Shutdown timed out")), SHUTDOWN_TIMEOUT_MS),
                ),
              ]);
            } catch {
              info.client.dispose();
            }
          }
          // Stop by server_id so multi-root workspaces stop the right server.
          if (info.serverId) {
            await invoke("lsp_stop", { serverId: info.serverId }).catch(() => {});
          }
        });

        await Promise.allSettled(shutdownPromises);

        cleanupProgressForWorkspace(workspacePath);
        cleanupRestartStateForWorkspace(workspacePath);

        set((state) => {
          delete state.servers[workspacePath];
          delete state.availability[workspacePath];
          delete state.indexProgress[workspacePath];
        });
      },
    };
  }),
);
