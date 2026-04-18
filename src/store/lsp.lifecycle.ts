import type { WritableDraft } from "immer";
import type { IndexProgress, LspState } from "./lsp.types";

export const SHUTDOWN_TIMEOUT_MS = 5_000;

/** Maximum restart attempts before giving up. */
const MAX_RESTART_ATTEMPTS = 5;
/** Base delay for exponential backoff in milliseconds. */
const BASE_RESTART_DELAY_MS = 1_000;
/** Maximum delay cap for exponential backoff in milliseconds. */
const MAX_RESTART_DELAY_MS = 30_000;

// Key: "workspacePath\0serverLang\0token"
export const progressEntries = new Map<
  string,
  { progress: IndexProgress; timer: ReturnType<typeof setTimeout> | null }
>();
/** Discard progress tokens whose server never emits "end". */
export const PROGRESS_TIMEOUT_MS = 5 * 60 * 1000;

export function progressKey(
  workspacePath: string,
  serverLang: string,
  token: string | number,
): string {
  return `${workspacePath}\0${serverLang}\0${token}`;
}

// Key: "workspacePath:serverLang"
export const restartState = new Map<string, { attempts: number; startTimestamp: number }>();

type SetFn = (fn: (state: WritableDraft<LspState>) => void) => void;
type GetFn = () => LspState;

const pendingSyncWorkspaces = new Set<string>();
let syncTimer: ReturnType<typeof setTimeout> | null = null;

export function flushProgressSync(set: SetFn) {
  syncTimer = null;
  const workspaces = [...pendingSyncWorkspaces];
  pendingSyncWorkspaces.clear();
  set((state) => {
    for (const wp of workspaces) {
      const prefix = wp + "\0";
      const entries: IndexProgress[] = [];
      for (const [key, { progress }] of progressEntries) {
        if (key.startsWith(prefix)) {
          entries.push(progress);
        }
      }
      state.indexProgress[wp] = entries;
    }
  });
}

export function syncProgressState(workspacePath: string, set: SetFn) {
  pendingSyncWorkspaces.add(workspacePath);
  if (!syncTimer) {
    syncTimer = setTimeout(() => flushProgressSync(set), 200);
  }
}

export function cleanupProgressForWorkspace(workspacePath: string) {
  pendingSyncWorkspaces.delete(workspacePath);
  const prefix = workspacePath + "\0";
  for (const [key, entry] of progressEntries) {
    if (key.startsWith(prefix)) {
      if (entry.timer) clearTimeout(entry.timer);
      progressEntries.delete(key);
    }
  }
}

export function cleanupRestartStateForWorkspace(workspacePath: string) {
  const restartPrefix = workspacePath + ":";
  for (const key of restartState.keys()) {
    if (key.startsWith(restartPrefix)) restartState.delete(key);
  }
}

export function handleServerStopped(
  workspacePath: string,
  serverLang: string,
  monacoRef: import("@monaco-editor/react").Monaco | null,
  set: SetFn,
  get: GetFn,
  error?: string | null,
) {
  const info = get().servers[workspacePath]?.[serverLang];
  if (info && info.status === "running") {
    set((state) => {
      const server = state.servers[workspacePath]?.[serverLang];
      if (server) {
        server.status = "stopped";
        server.errorMessage = error ?? null;
      }
    });

    if (!monacoRef) return;

    const key = `${workspacePath}:${serverLang}`;
    const rs = restartState.get(key);
    let attempts = rs?.attempts ?? 0;

    // Reset the backoff if the server stayed up >60s — it was stable.
    if (rs && Date.now() - rs.startTimestamp > 60_000) attempts = 0;

    if (attempts >= MAX_RESTART_ATTEMPTS) {
      console.error(
        `[kosmos:lsp] Server ${serverLang} stopped. Max restart attempts (${MAX_RESTART_ATTEMPTS}) reached.`,
      );
      restartState.delete(key);
      return;
    }

    restartState.set(key, {
      attempts: attempts + 1,
      startTimestamp: rs?.startTimestamp ?? Date.now(),
    });

    const monaco = monacoRef;
    const delay = Math.round(
      Math.min(BASE_RESTART_DELAY_MS * 2 ** attempts, MAX_RESTART_DELAY_MS) *
        (0.5 + Math.random() * 0.5),
    );

    console.warn(
      `[kosmos:lsp] Server ${serverLang} stopped unexpectedly. ` +
        `Restart attempt ${attempts + 1}/${MAX_RESTART_ATTEMPTS} in ${delay}ms...`,
    );

    setTimeout(() => {
      set((s) => {
        const ws = s.servers[workspacePath];
        if (ws) delete ws[serverLang];
      });

      get()
        .startServer(workspacePath, serverLang, null, monaco)
        .then((client) => {
          if (client) {
            restartState.set(key, { attempts: 0, startTimestamp: Date.now() });
            console.info(`[kosmos:lsp] Server ${serverLang} restarted successfully.`);
          }
        })
        .catch((err) => {
          console.error(`[kosmos:lsp] Restart failed for ${serverLang}:`, err);
        });
    }, delay);
  }
}
