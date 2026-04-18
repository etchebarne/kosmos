import type { WritableDraft } from "immer";
import type { Monaco } from "@monaco-editor/react";
import { invoke } from "@tauri-apps/api/core";
import { TauriLspTransport } from "../lib/lsp/transport";
import { LspClient } from "../lib/lsp/client";
import { registerLspProviders } from "../lib/lsp/monacoBridge";
import type { IndexProgress, LspServerInfo, LspState } from "./lsp.types";
import {
  progressEntries,
  progressKey,
  PROGRESS_TIMEOUT_MS,
  restartState,
  flushProgressSync,
  syncProgressState,
  handleServerStopped,
} from "./lsp.lifecycle";

type SetFn = (fn: (state: WritableDraft<LspState>) => void) => void;
type GetFn = () => LspState;

// Backend is authoritative for language groups and companion mapping.
let languageGroupMap: Record<string, string> | null = null;
/** companion language → list of primary server language groups it serves */
let companionMap: Record<string, string[]> | null = null;

export async function ensureLanguageGroups(): Promise<void> {
  if (languageGroupMap) return;
  try {
    const [groups, companions] = await Promise.all([
      invoke<Record<string, string>>("lsp_language_groups"),
      invoke<Record<string, string[]>>("lsp_companion_servers"),
    ]);
    languageGroupMap = groups;
    companionMap = companions;
  } catch (err) {
    console.warn("[kosmos:lsp] Failed to load language groups:", err);
    languageGroupMap = {};
    companionMap = {};
  }
}

export function resolveServerLanguage(languageId: string): string {
  return languageGroupMap?.[languageId] ?? languageId;
}

function getMonacoLanguages(serverLanguage: string): string[] {
  const languages = [serverLanguage];
  if (languageGroupMap) {
    for (const [lang, group] of Object.entries(languageGroupMap)) {
      if (group === serverLanguage && !languages.includes(lang)) {
        languages.push(lang);
      }
    }
  }
  return languages;
}

/** Returns companion server language IDs that should run alongside a primary server. */
export function getCompanionsFor(primaryServerLang: string): string[] {
  if (!companionMap) return [];
  const companions: string[] = [];
  for (const [companionLang, targets] of Object.entries(companionMap)) {
    if (targets.includes(primaryServerLang)) {
      companions.push(companionLang);
    }
  }
  return companions;
}

/** Returns true if this server language is a companion (not a primary). */
function isCompanionServer(serverLang: string): boolean {
  return companionMap != null && serverLang in companionMap;
}

/** For a companion server, returns all Monaco language IDs it should provide features for. */
function getCompanionTargetLanguages(companionLang: string): string[] {
  const targets = companionMap?.[companionLang];
  if (!targets) return [];
  const langs: string[] = [];
  for (const group of targets) {
    for (const lang of getMonacoLanguages(group)) {
      if (!langs.includes(lang)) langs.push(lang);
    }
  }
  return langs;
}

// Concurrent starts for the same server share this promise.
export const pending = new Map<string, Promise<LspClient | null>>();

export function setServerInfo(
  workspacePath: string,
  serverLang: string,
  info: LspServerInfo,
  set: SetFn,
) {
  set((state) => {
    if (!state.servers[workspacePath]) {
      state.servers[workspacePath] = {};
    }
    state.servers[workspacePath][serverLang] = info;
  });
}

export function ensureProviders(
  workspacePath: string,
  serverLang: string,
  info: LspServerInfo,
  monaco: Monaco,
  set: SetFn,
) {
  if (info.providerDisposables.length > 0 || !info.client) return;

  // Companions provide features for the languages they serve, not their own id.
  const monacoLangs = isCompanionServer(serverLang)
    ? getCompanionTargetLanguages(serverLang)
    : getMonacoLanguages(serverLang);
  const providerDisposables = registerLspProviders(monaco, info.client, monacoLangs);

  if (serverLang === "typescript") {
    monaco.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
      noSemanticValidation: true,
      noSyntaxValidation: true,
    });
    monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
      noSemanticValidation: true,
      noSyntaxValidation: true,
    });
  }

  set((state) => {
    const server = state.servers[workspacePath]?.[serverLang];
    if (server) {
      server.providerDisposables = providerDisposables;
    }
  });
}

export async function initializeServer(
  workspacePath: string,
  projectRoot: string,
  languageId: string,
  monacoRef: Monaco | null,
  set: SetFn,
  get: GetFn,
): Promise<LspClient | null> {
  await ensureLanguageGroups();
  const serverLang = resolveServerLanguage(languageId);

  const existing = get().servers[workspacePath]?.[serverLang];
  if (existing && (existing.status === "running" || existing.status === "starting")) {
    return existing.status === "running" ? existing.client : null;
  }
  if (existing?.status === "unavailable" || existing?.status === "installing") {
    return null;
  }

  const pendingKey = `${workspacePath}:${serverLang}`;
  const inflight = pending.get(pendingKey);
  if (inflight) return inflight;

  const promise = (async (): Promise<LspClient | null> => {
    const result = await invoke<{
      serverId: string;
      serverName: string;
      serverLanguage: string;
    }>("lsp_start", {
      workspacePath: projectRoot,
      languageId,
    });

    const transport = new TauriLspTransport(result.serverId);
    await transport.connect();
    // For wsl://distro/path roots, pass the prefix so the client can rewrite URIs
    // between editor paths and the agent's native Linux paths.
    const wslMatch = projectRoot.match(/^(wsl:\/\/[^/]+)/);
    const client = new LspClient(transport, wslMatch?.[1]);

    transport.onServerStopped((error) =>
      handleServerStopped(workspacePath, serverLang, monacoRef, set, get, error),
    );

    try {
      await client.initialize(projectRoot);
    } catch (err) {
      // Re-throw with the server name so callers can attribute the failure.
      await invoke("lsp_stop", { serverId: result.serverId }).catch(() => {});
      throw new Error(
        `Failed to start ${result.serverName}: ${err instanceof Error ? err.message : err}`,
      );
    }

    client.onProgress((token, value) => {
      const key = progressKey(workspacePath, serverLang, token);
      if (value.kind === "begin") {
        const entry = progressEntries.get(key);
        if (entry?.timer) clearTimeout(entry.timer);
        const progress: IndexProgress = {
          serverName: result.serverName,
          title: value.title,
          message: value.message,
          percentage: value.percentage,
        };
        progressEntries.set(key, {
          progress,
          timer: setTimeout(() => {
            progressEntries.delete(key);
            syncProgressState(workspacePath, set);
          }, PROGRESS_TIMEOUT_MS),
        });
        // Flush so the indicator shows/hides immediately rather than at next batch.
        flushProgressSync(set);
      } else if (value.kind === "report") {
        const entry = progressEntries.get(key);
        if (entry) {
          entry.progress = {
            ...entry.progress,
            message: value.message ?? entry.progress.message,
            percentage: value.percentage ?? entry.progress.percentage,
          };
        }
        syncProgressState(workspacePath, set);
      } else if (value.kind === "end") {
        const entry = progressEntries.get(key);
        if (entry?.timer) clearTimeout(entry.timer);
        progressEntries.delete(key);
        flushProgressSync(set);
      }
    });

    const startKey = `${workspacePath}:${serverLang}`;
    restartState.set(startKey, { attempts: 0, startTimestamp: Date.now() });

    setServerInfo(
      workspacePath,
      serverLang,
      {
        serverId: result.serverId,
        languageId: serverLang,
        client,
        status: "running",
        serverName: result.serverName,
        errorMessage: null,
        providerDisposables: [],
      },
      set,
    );

    return client;
  })();

  pending.set(pendingKey, promise);
  promise.finally(() => pending.delete(pendingKey));
  return promise;
}
