import type {
  PaneId,
  PaneNodeSnapshot,
  PaneSnapshot,
  TabId,
  TabKind,
  SplitPaneId,
  WorkspaceId,
  WorkspaceListSnapshot,
  WorkspaceSnapshot,
} from "@/shared/ipc";

export function activeWorkspaceFrom(
  snapshot: WorkspaceListSnapshot | null,
): WorkspaceSnapshot | null {
  if (!snapshot?.activeWorkspaceId) {
    return null;
  }

  return (
    snapshot.workspaces.find((workspace) => workspace.id === snapshot.activeWorkspaceId) ?? null
  );
}

export function workspacePanes(workspace: WorkspaceSnapshot): PaneSnapshot[] {
  const panes: PaneSnapshot[] = [];
  collectPanes(workspace.root, panes);
  return panes;
}

export function activePaneOf(workspace: WorkspaceSnapshot): PaneSnapshot | null {
  return workspacePanes(workspace).find((pane) => pane.id === workspace.activePaneId) ?? null;
}

/** Returns the tab `offset` positions away from the active one, wrapping around. */
export function adjacentTabId(pane: PaneSnapshot, offset: number): TabId | null {
  const count = pane.tabs.length;
  const activeIndex = pane.tabs.findIndex((tab) => tab.id === pane.activeTabId);
  if (count < 2 || activeIndex === -1) {
    return null;
  }

  return pane.tabs[(((activeIndex + offset) % count) + count) % count]!.id;
}

/** Finds a tab of `kind`, preferring the active pane and its active tab. */
export function findTabOfKind(
  workspace: WorkspaceSnapshot,
  kind: TabKind,
): { paneId: PaneId; tabId: TabId } | null {
  const panes = workspacePanes(workspace);
  const ordered = [
    ...panes.filter((pane) => pane.id === workspace.activePaneId),
    ...panes.filter((pane) => pane.id !== workspace.activePaneId),
  ];

  for (const pane of ordered) {
    const tab = pane.tabs.find((candidate) => candidate.id === pane.activeTabId && candidate.kind === kind)
      ?? pane.tabs.find((candidate) => candidate.kind === kind);
    if (tab) {
      return { paneId: pane.id, tabId: tab.id };
    }
  }

  return null;
}

export function closeWorkspaceLocally(
  snapshot: WorkspaceListSnapshot | null,
  workspaceId: WorkspaceId,
): WorkspaceListSnapshot | null {
  if (!snapshot) {
    return snapshot;
  }

  const workspaceIndex = snapshot.workspaces.findIndex((workspace) => workspace.id === workspaceId);

  if (workspaceIndex === -1) {
    return snapshot;
  }

  const workspaces = snapshot.workspaces.filter((workspace) => workspace.id !== workspaceId);
  const activeWorkspaceId =
    snapshot.activeWorkspaceId === workspaceId
      ? (workspaces[workspaceIndex]?.id ?? workspaces[workspaceIndex - 1]?.id ?? null)
      : snapshot.activeWorkspaceId;

  return { ...snapshot, activeWorkspaceId, workspaces };
}

export function moveWorkspaceLocally(
  snapshot: WorkspaceListSnapshot | null,
  workspaceId: WorkspaceId,
  targetIndex: number,
): WorkspaceListSnapshot | null {
  if (!snapshot || !Number.isSafeInteger(targetIndex) || targetIndex < 0) {
    return snapshot;
  }

  const currentIndex = snapshot.workspaces.findIndex((workspace) => workspace.id === workspaceId);
  if (currentIndex === -1) {
    return snapshot;
  }

  const insertionIndex = Math.min(targetIndex, snapshot.workspaces.length);
  const nextIndex = insertionIndex > currentIndex ? insertionIndex - 1 : insertionIndex;
  if (nextIndex === currentIndex) {
    return snapshot;
  }

  const workspaces = [...snapshot.workspaces];
  const workspace = workspaces.splice(currentIndex, 1)[0]!;
  workspaces.splice(nextIndex, 0, workspace);

  return { ...snapshot, workspaces };
}

export function resizeSplitLocally(
  snapshot: WorkspaceListSnapshot | null,
  workspaceId: WorkspaceId,
  splitId: SplitPaneId,
  ratio: number,
): WorkspaceListSnapshot | null {
  if (!snapshot || !isValidSplitRatio(ratio)) {
    return snapshot;
  }

  let updated = false;
  const workspaces = snapshot.workspaces.map((workspace) => {
    if (workspace.id !== workspaceId) {
      return workspace;
    }

    const root = resizeNodeSplit(workspace.root, splitId, ratio);
    if (root === workspace.root) {
      return workspace;
    }

    updated = true;
    return { ...workspace, root };
  });

  return updated ? { ...snapshot, workspaces } : snapshot;
}

export function mergeLocalSplitRatios(
  snapshot: WorkspaceListSnapshot,
  localSnapshot: WorkspaceListSnapshot | null,
): WorkspaceListSnapshot {
  if (!localSnapshot) {
    return snapshot;
  }

  const ratiosByWorkspace = new Map<WorkspaceId, Map<SplitPaneId, number>>();
  for (const workspace of localSnapshot.workspaces) {
    const ratios = new Map<SplitPaneId, number>();
    collectSplitRatios(workspace.root, ratios);
    ratiosByWorkspace.set(workspace.id, ratios);
  }

  let updated = false;
  const workspaces = snapshot.workspaces.map((workspace) => {
    const ratios = ratiosByWorkspace.get(workspace.id);
    if (!ratios) {
      return workspace;
    }

    const root = applySplitRatios(workspace.root, ratios);
    if (root === workspace.root) {
      return workspace;
    }

    updated = true;
    return { ...workspace, root };
  });

  return updated ? { ...snapshot, workspaces } : snapshot;
}

/** Lists `workspaceId:tabId` keys of tabs present in `previous` but gone from `next`. */
export function closedTabKeys(
  previous: WorkspaceListSnapshot | null,
  next: WorkspaceListSnapshot,
): string[] {
  if (!previous) {
    return [];
  }
  const nextKeys = openTabKeys(next);
  return [...openTabKeys(previous)].filter((key) => !nextKeys.has(key));
}

function openTabKeys(snapshot: WorkspaceListSnapshot): Set<string> {
  const keys = new Set<string>();
  for (const workspace of snapshot.workspaces) {
    collectTabKeys(workspace.id, workspace.root, keys);
  }
  return keys;
}

function collectTabKeys(workspaceId: number, node: PaneNodeSnapshot, keys: Set<string>): void {
  const panes: PaneSnapshot[] = [];
  collectPanes(node, panes);
  for (const tab of panes.flatMap((pane) => pane.tabs)) {
    keys.add(`${workspaceId}:${tab.id}`);
  }
}

function collectPanes(node: PaneNodeSnapshot, panes: PaneSnapshot[]): void {
  if (node.type === "leaf") {
    panes.push(node.pane);
    return;
  }

  collectPanes(node.first, panes);
  collectPanes(node.second, panes);
}

function resizeNodeSplit(
  node: PaneNodeSnapshot,
  splitId: SplitPaneId,
  ratio: number,
): PaneNodeSnapshot {
  if (node.type === "leaf") {
    return node;
  }

  if (node.id === splitId) {
    return node.ratio === ratio ? node : { ...node, ratio };
  }

  const first = resizeNodeSplit(node.first, splitId, ratio);
  if (first !== node.first) {
    return { ...node, first };
  }

  const second = resizeNodeSplit(node.second, splitId, ratio);
  if (second !== node.second) {
    return { ...node, second };
  }

  return node;
}

function isValidSplitRatio(ratio: number): boolean {
  return Number.isFinite(ratio) && ratio > 0 && ratio < 1;
}

function collectSplitRatios(node: PaneNodeSnapshot, ratios: Map<SplitPaneId, number>): void {
  if (node.type === "leaf") {
    return;
  }

  ratios.set(node.id, node.ratio);
  collectSplitRatios(node.first, ratios);
  collectSplitRatios(node.second, ratios);
}

function applySplitRatios(
  node: PaneNodeSnapshot,
  ratios: Map<SplitPaneId, number>,
): PaneNodeSnapshot {
  if (node.type === "leaf") {
    return node;
  }

  const ratio = ratios.get(node.id);
  const first = applySplitRatios(node.first, ratios);
  const second = applySplitRatios(node.second, ratios);
  const nextRatio = ratio ?? node.ratio;

  if (first === node.first && second === node.second && nextRatio === node.ratio) {
    return node;
  }

  return { ...node, first, second, ratio: nextRatio };
}
