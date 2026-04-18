import type { ComponentType } from "react";
import type { TabContentProps } from "../tabs/types";

/** Static JSON in each plugin directory. */
export interface PluginManifest {
  name: string;
  version: string;
  description?: string;
  author?: string;
  /** Minimum kosmos version required (semver) */
  engine?: string;
  /** Entry point relative to the plugin directory */
  main: string;
  contributes?: PluginContributions;
}

export interface PluginContributions {
  tabs?: PluginTabContribution[];
  commands?: PluginCommandContribution[];
}

export interface PluginTabContribution {
  type: string;
  title: string;
  icon: string;
  defaultSize?: { width: number; height: number };
}

export interface PluginCommandContribution {
  id: string;
  title: string;
}

export interface InstalledPlugin {
  /** Store key — derived from the plugin's directory name */
  pluginId: string;
  manifest: PluginManifest;
  /** Absolute path to the plugin directory */
  path: string;
  enabled: boolean;
  activated: boolean;
}

/** Exported by each plugin's JS entry. */
export interface PluginModule {
  activate(api: KosmosPluginAPI): void | Promise<void>;
  deactivate?(): void | Promise<void>;
}

export interface Disposable {
  dispose(): void;
}

export interface KosmosPluginAPI {
  tabs: {
    register(definition: {
      type: string;
      title: string;
      icon: string;
      component: ComponentType<TabContentProps>;
      hidden?: boolean;
      defaultSize?: { width: number; height: number };
    }): Disposable;
    open(type: string, metadata?: Record<string, unknown>): void;
  };

  commands: {
    register(id: string, handler: () => void): Disposable;
    execute(id: string): void;
  };

  fs: {
    readFile(path: string): Promise<string>;
    writeFile(path: string, content: string): Promise<void>;
  };

  /** Run commands and spawn long-running processes. */
  shell: {
    /** Execute a command and return its output once it finishes. */
    execute(
      command: string,
      args?: string[],
      opts?: { cwd?: string },
    ): Promise<{ stdout: string; stderr: string; code: number }>;
    /** Spawn a long-running child process with streaming I/O. */
    spawn(command: string, args?: string[], opts?: { cwd?: string }): Promise<ChildProcess>;
  };

  /** Pub/sub event bus shared across all plugins. */
  events: {
    /** Emit an event visible to all plugins. */
    emit(event: string, data?: unknown): void;
    /** Subscribe to an event. */
    on(event: string, cb: (data: unknown) => void): Disposable;
  };

  ui: {
    showNotification(message: string, type?: "info" | "error" | "success"): void;
  };
}

/** Handle to a spawned child process. */
export interface ChildProcess {
  /** Unique process ID (internal, for tracking). */
  readonly pid: string;
  /** Write to the process's stdin. */
  write(data: string): Promise<void>;
  /** Subscribe to stdout chunks. */
  onStdout(cb: (data: string) => void): Disposable;
  /** Subscribe to stderr chunks. */
  onStderr(cb: (data: string) => void): Disposable;
  /** Subscribe to process exit. */
  onExit(cb: (code: number) => void): Disposable;
  /** Kill the process. */
  kill(): Promise<void>;
}

/** Entry in the curated marketplace registry. */
export interface RegistryEntry {
  name: string;
  version: string;
  description?: string;
  author?: string;
  /** URL to download the plugin archive (.tar.gz or .zip) */
  download: string;
  /** URL to the plugin's homepage or repo */
  homepage?: string;
  icon?: string;
}

/** Derive a stable ID from a registry entry (author.name-slug). */
export function registryEntryId(entry: RegistryEntry): string {
  const slug = entry.name.toLowerCase().replace(/\s+/g, "-");
  return entry.author ? `${entry.author}.${slug}` : slug;
}
