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

export type { ServerStatus, ServerAvailability, LspServerInfo, IndexProgress } from "./lsp.types";
export { resolveServerLanguage } from "./lsp.init";

// Store monaco instance for restarting servers after install
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

        // Deep-scan the workspace tree for project markers (Cargo.toml, package.json, etc.)
        // at any depth, with each project resolved to its correct root directory.
        let projects: { languageId: string; projectRoot: string; available: boolean }[];
        try {
          projects = await invoke<typeof projects>("lsp_scan_projects", { workspacePath });
        } catch {
          return;
        }

        // Start each available server concurrently in the background.
        // Each server gets the resolved project root as its cwd and rootUri.
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

        // If server is already running (e.g. from warmup), ensure providers are registered
        const existing = get().servers[workspacePath]?.[serverLang];
        if (existing && existing.status === "running") {
          ensureProviders(workspacePath, serverLang, existing, monaco, set);
          return existing.client;
        }

        // Don't retry if we know it's unavailable or installing
        if (existing?.status === "unavailable" || existing?.status === "installing") {
          return null;
        }

        // Resolve the actual project root: walk up from the file to find the
        // nearest project marker (Cargo.toml, package.json, etc.)
        let projectRoot = workspacePath;
        if (filePath) {
          try {
            projectRoot = await invoke<string>("lsp_resolve_root", {
              filePath,
              languageId,
              workspacePath,
            });
          } catch {
            // Fall back to workspace root
          }
        }

        try {
          // May share an in-flight promise from warmupWorkspace or another startServer call
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

          // No server configured for this language — silently ignore
          if (errorStr.includes("No language server configured")) {
            return null;
          }

          // Detect "not found" errors (binary not on PATH or shim can't resolve)
          const isNotFound =
            /not found|No such file|program not found|os error 2|cannot find|Unknown binary/i.test(
              errorStr,
            );

          // Server binary existed but crashed immediately during init
          // (e.g. rustup proxy when component isn't installed)
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

          // Guard: another concurrent startServer may have already handled this
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
          if (existing) continue; // already started or starting

          // Start the companion server (reuses the same initializeServer flow)
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

        // Shutdown all servers in parallel
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
          // Stop the specific server on the backend by ID (handles resolved roots correctly)
          if (info.serverId) {
            await invoke("lsp_stop", { serverId: info.serverId }).catch(() => {});
          }
        });

        await Promise.allSettled(shutdownPromises);

        // Clean up progress entries and pending sync for this workspace
        cleanupProgressForWorkspace(workspacePath);

        // Clean up restart state tracking
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
