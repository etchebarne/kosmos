import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { registerTab, unregisterTab } from "../tabs/registry";
import { useLayoutStore } from "../store/layout.store";
import { useToastStore } from "../store/toast.store";
import { findAllLeaves } from "../lib/paneTree";
import type { KosmosPluginAPI, Disposable, ChildProcess } from "./types";
import type { TabDefinition } from "../tabs/types";

const commands = new Map<string, () => void>();

type EventCallback = (data: unknown) => void;
const eventBus = new Map<string, Set<EventCallback>>();

let processIdCounter = 0;

/** One API instance per plugin so disposables can be tracked per-plugin. */
export function createPluginAPI(pluginId: string): {
  api: KosmosPluginAPI;
  disposables: Disposable[];
} {
  const disposables: Disposable[] = [];

  const api: KosmosPluginAPI = {
    tabs: {
      register(definition) {
        const fullDef: TabDefinition = { ...definition };
        registerTab(fullDef);
        const d: Disposable = { dispose: () => unregisterTab(definition.type) };
        disposables.push(d);
        return d;
      },

      open(type, metadata) {
        const layout = useLayoutStore.getState();
        const leaves = findAllLeaves(layout.layout);
        const targetPane = leaves[0];
        if (targetPane) {
          layout.addTab(targetPane.id, type, undefined, metadata);
        }
      },
    },

    commands: {
      register(id, handler) {
        const qualifiedId = `${pluginId}.${id}`;
        commands.set(qualifiedId, handler);
        const d: Disposable = {
          dispose: () => {
            commands.delete(qualifiedId);
          },
        };
        disposables.push(d);
        return d;
      },

      execute(id) {
        const handler = commands.get(id);
        if (handler) handler();
      },
    },

    fs: {
      async readFile(path: string) {
        return invoke<string>("read_file", { path });
      },

      async writeFile(path: string, content: string) {
        await invoke("write_file", { path, content });
      },
    },

    shell: {
      async execute(command, args = [], opts = {}) {
        return invoke<{ stdout: string; stderr: string; code: number }>("plugin_shell_execute", {
          command,
          args,
          cwd: opts.cwd ?? null,
        });
      },

      async spawn(command, args = [], opts = {}): Promise<ChildProcess> {
        const pid = `${pluginId}-${++processIdCounter}`;

        await invoke("plugin_shell_spawn", {
          pid,
          command,
          args,
          cwd: opts.cwd ?? null,
        });

        const stdoutCbs = new Set<(data: string) => void>();
        const stderrCbs = new Set<(data: string) => void>();
        const exitCbs = new Set<(code: number) => void>();

        const unlistenStdout = await listen<string>(`plugin-process-stdout-${pid}`, (e) =>
          stdoutCbs.forEach((cb) => cb(e.payload)),
        );
        const unlistenStderr = await listen<string>(`plugin-process-stderr-${pid}`, (e) =>
          stderrCbs.forEach((cb) => cb(e.payload)),
        );
        const unlistenExit = await listen<number>(`plugin-process-exit-${pid}`, (e) => {
          exitCbs.forEach((cb) => cb(e.payload));
          unlistenStdout();
          unlistenStderr();
          unlistenExit();
        });

        disposables.push({
          dispose: () => {
            unlistenStdout();
            unlistenStderr();
            unlistenExit();
            invoke("plugin_shell_kill", { pid }).catch(() => {});
          },
        });

        const childProcess: ChildProcess = {
          pid,

          async write(data: string) {
            await invoke("plugin_shell_write", { pid, data });
          },

          onStdout(cb) {
            stdoutCbs.add(cb);
            return { dispose: () => stdoutCbs.delete(cb) };
          },

          onStderr(cb) {
            stderrCbs.add(cb);
            return { dispose: () => stderrCbs.delete(cb) };
          },

          onExit(cb) {
            exitCbs.add(cb);
            return { dispose: () => exitCbs.delete(cb) };
          },

          async kill() {
            await invoke("plugin_shell_kill", { pid });
          },
        };

        return childProcess;
      },
    },

    events: {
      emit(event, data) {
        const listeners = eventBus.get(event);
        if (listeners) {
          listeners.forEach((cb) => cb(data));
        }
      },

      on(event, cb) {
        let listeners = eventBus.get(event);
        if (!listeners) {
          listeners = new Set();
          eventBus.set(event, listeners);
        }
        listeners.add(cb);

        const d: Disposable = {
          dispose: () => {
            listeners!.delete(cb);
            if (listeners!.size === 0) eventBus.delete(event);
          },
        };
        disposables.push(d);
        return d;
      },
    },

    ui: {
      showNotification(message, type = "info") {
        useToastStore.getState().addToast({ message, type });
      },
    },
  };

  return { api, disposables };
}
