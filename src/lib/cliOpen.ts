import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useWorkspaceStore } from "../store/workspace.store";
import { useLayoutStore } from "../store/layout.store";
import { getFileName, getParentDir, normalizePath } from "./pathUtils";
import { findAllLeaves } from "./paneTree";

type OpenPath = { path: string; kind: "file" | "dir" };

/** Find an already-open workspace whose directory contains the given file. */
function findContainingWorkspaceIndex(path: string): number {
  const target = normalizePath(path);
  const { workspaces } = useWorkspaceStore.getState();
  let bestIdx = -1;
  let bestLen = -1;
  for (let i = 0; i < workspaces.length; i++) {
    const wsPath = normalizePath(workspaces[i].path);
    if (target === wsPath || target.startsWith(wsPath + "/")) {
      if (wsPath.length > bestLen) {
        bestIdx = i;
        bestLen = wsPath.length;
      }
    }
  }
  return bestIdx;
}

async function openSingle({ path, kind }: OpenPath) {
  const wsStore = useWorkspaceStore.getState();

  if (kind === "dir") {
    await wsStore.openWorkspace(path);
    return;
  }

  const existingIdx = findContainingWorkspaceIndex(path);
  let workspacePath: string;
  if (existingIdx !== -1) {
    if (useWorkspaceStore.getState().activeIndex !== existingIdx) {
      await wsStore.switchWorkspace(existingIdx);
    }
    workspacePath = useWorkspaceStore.getState().workspaces[existingIdx].path;
  } else {
    workspacePath = getParentDir(path);
    await wsStore.openWorkspace(workspacePath);
  }

  // Sync layout store so `openFile` writes into the correct workspace's layout.
  const layout = useLayoutStore.getState();
  if (layout.activeWorkspacePath !== workspacePath) {
    layout.setWorkspace(workspacePath);
  }

  const current = useLayoutStore.getState();
  const paneId = current.activePaneId ?? findAllLeaves(current.layout)[0]?.id ?? "";
  current.openFile(path, getFileName(path), paneId);
}

async function processOpenPaths(paths: OpenPath[]) {
  for (const p of paths) {
    try {
      await openSingle(p);
    } catch (e) {
      console.warn("Failed to open path from CLI:", p, e);
    }
  }
}

/** Drain startup argv and listen for secondary-instance opens. */
export async function initCliOpen(): Promise<void> {
  listen<OpenPath[]>("open-files", (event) => {
    void processOpenPaths(event.payload);
  });

  try {
    const pending = await invoke<OpenPath[]>("take_pending_open_files");
    if (pending.length > 0) await processOpenPaths(pending);
  } catch (e) {
    console.warn("take_pending_open_files failed:", e);
  }
}
