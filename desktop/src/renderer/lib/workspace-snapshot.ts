import type {
  PaneNodeSnapshot,
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
