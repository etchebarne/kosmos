import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { getTauriStore } from "../lib/tauriStore";
import { useLspStore } from "./lsp.store";
import { cleanupEditorInstances } from "../tabs/editor/editorCache";
import { getFileName } from "../lib/pathUtils";

const WORKSPACE_COLORS = [
  "#4B8EF5",
  "#7C5CED",
  "#10B981",
  "#E84757",
  "#F59E0B",
  "#EC4899",
  "#06B6D4",
  "#8B5CF6",
];

export type ConnectionType =
  | { type: "local" }
  | { type: "wsl"; distro: string }
  | { type: "ssh"; host: string; user?: string };

export interface Workspace {
  path: string;
  name: string;
  color: string;
  connection: ConnectionType;
  avatarUrl?: string | null;
}

interface WorkspaceStore {
  workspaces: Workspace[];
  activeIndex: number | null;
  ready: boolean;
  /** Paths of remote workspaces that are still connecting. */
  connectingPaths: Set<string>;

  init: () => Promise<void>;
  openWorkspace: (path: string, connection?: ConnectionType) => Promise<void>;
  switchWorkspace: (index: number) => Promise<void>;
  closeWorkspace: (index: number) => Promise<void>;
  reorderWorkspace: (fromIndex: number, toIndex: number) => void;
  isConnecting: (path: string) => boolean;
}

function nameFromPath(path: string): string {
  return getFileName(path) || "workspace";
}

async function persist(workspaces: Workspace[], activeIndex: number | null) {
  const s = await getTauriStore("workspace.json");
  await s.set("workspaces", workspaces);
  await s.set("activeIndex", activeIndex);
}

/** Try to resolve a GitHub avatar URL for a workspace. */
async function fetchAvatarUrl(path: string): Promise<string | null> {
  try {
    const owner = await invoke<string | null>("get_git_remote_owner", { path });
    if (owner) return `https://github.com/${owner}.png?size=64`;
  } catch {
    // Not a git repo or no remote.
  }
  return null;
}

/** Resolve avatars for workspaces that don't have one yet, then persist. */
async function resolveAvatars() {
  const { workspaces } = useWorkspaceStore.getState();
  await Promise.all(
    workspaces
      .filter((w) => w.avatarUrl === undefined)
      .map(async (w) => {
        const avatarUrl = await fetchAvatarUrl(w.path);
        const current = useWorkspaceStore.getState();
        const idx = current.workspaces.findIndex((ws) => ws.path === w.path);
        if (idx === -1) return;
        const updated = [...current.workspaces];
        updated[idx] = { ...updated[idx], avatarUrl };
        persist(updated, current.activeIndex);
        useWorkspaceStore.setState({ workspaces: updated });
      }),
  );
}

export const useWorkspaceStore = create<WorkspaceStore>((set, get) => ({
  workspaces: [],
  activeIndex: null,
  ready: false,
  connectingPaths: new Set(),

  isConnecting: (path: string) => get().connectingPaths.has(path),

  init: async () => {
    const s = await getTauriStore("workspace.json");
    const raw = (await s.get<Workspace[]>("workspaces")) ?? [];
    // Default connection for pre-remote saves.
    const workspaces = raw.map((w) => ({
      ...w,
      connection: w.connection ?? { type: "local" as const },
    }));
    const activeIndex = (await s.get<number>("activeIndex")) ?? null;
    const remotePaths = new Set(
      workspaces.filter((w) => w.connection.type !== "local").map((w) => w.path),
    );

    set({ workspaces, activeIndex, ready: true, connectingPaths: remotePaths });

    resolveAvatars();

    for (const w of workspaces) {
      if (w.connection.type !== "local") {
        invoke("remote_ensure_connected", {
          workspacePath: w.path,
          connection: w.connection,
        })
          .catch((e) => {
            console.warn(`Failed to reconnect ${w.name}:`, e);
          })
          .finally(() => {
            const next = new Set(get().connectingPaths);
            next.delete(w.path);
            set({ connectingPaths: next });
          });
      }
    }
  },

  openWorkspace: async (path: string, connection: ConnectionType = { type: "local" }) => {
    const state = get();
    const existing = state.workspaces.findIndex((w) => w.path === path);

    if (existing !== -1) {
      persist(state.workspaces, existing);
      set({ activeIndex: existing });
      return;
    }

    const color = WORKSPACE_COLORS[state.workspaces.length % WORKSPACE_COLORS.length];
    const workspace: Workspace = { path, name: nameFromPath(path), color, connection };
    const workspaces = [...state.workspaces, workspace];
    const activeIndex = workspaces.length - 1;
    persist(workspaces, activeIndex);
    set({ workspaces, activeIndex });

    resolveAvatars();
  },

  switchWorkspace: async (index: number) => {
    const state = get();
    if (index < 0 || index >= state.workspaces.length) return;
    persist(state.workspaces, index);
    set({ activeIndex: index });
  },

  reorderWorkspace: (fromIndex: number, toIndex: number) => {
    const state = get();
    if (
      fromIndex === toIndex ||
      fromIndex < 0 ||
      fromIndex >= state.workspaces.length ||
      toIndex < 0 ||
      toIndex >= state.workspaces.length
    )
      return;

    const workspaces = [...state.workspaces];
    const [moved] = workspaces.splice(fromIndex, 1);
    workspaces.splice(toIndex, 0, moved);

    let activeIndex = state.activeIndex;
    if (activeIndex !== null) {
      if (activeIndex === fromIndex) {
        activeIndex = toIndex;
      } else if (fromIndex < activeIndex && toIndex >= activeIndex) {
        activeIndex -= 1;
      } else if (fromIndex > activeIndex && toIndex <= activeIndex) {
        activeIndex += 1;
      }
    }

    persist(workspaces, activeIndex);
    set({ workspaces, activeIndex });
  },

  closeWorkspace: async (index: number) => {
    const state = get();
    const closedPath = state.workspaces[index]?.path;
    const workspaces = state.workspaces.filter((_, i) => i !== index);
    let activeIndex: number | null = state.activeIndex;

    if (closedPath) {
      useLspStore.getState().stopWorkspace(closedPath);
      cleanupEditorInstances(closedPath);
    }

    const closedWorkspace = state.workspaces[index];
    if (closedWorkspace?.connection?.type !== "local") {
      invoke("remote_disconnect", { workspacePath: closedPath }).catch((e) =>
        console.warn("remote_disconnect failed:", e),
      );
    }

    if (workspaces.length === 0) {
      activeIndex = null;
    } else if (activeIndex !== null) {
      if (index === activeIndex) {
        activeIndex = Math.min(index, workspaces.length - 1);
      } else if (index < activeIndex) {
        activeIndex = activeIndex - 1;
      }
    }

    persist(workspaces, activeIndex);
    set({ workspaces, activeIndex });
  },
}));
