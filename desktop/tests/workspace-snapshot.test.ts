import { describe, expect, test } from "bun:test";

import {
  activeWorkspaceFrom,
  closeWorkspaceLocally,
  mergeLocalSplitRatios,
  moveWorkspaceLocally,
  resizeSplitLocally,
} from "@/renderer/lib/workspace-snapshot";
import type {
  PaneNodeSnapshot,
  WorkspaceListSnapshot,
  WorkspaceSnapshot,
} from "@/shared/ipc";

function leaf(id: number): PaneNodeSnapshot {
  return {
    type: "leaf",
    pane: { id, activeTabId: id, tabs: [] },
  };
}

function split(id: number, ratio: number): PaneNodeSnapshot {
  return {
    type: "split",
    id,
    axis: "horizontal",
    ratio,
    first: leaf(id * 10),
    second: leaf(id * 10 + 1),
  };
}

function workspace(id: number, root: PaneNodeSnapshot = leaf(id)): WorkspaceSnapshot {
  return {
    id,
    name: `Workspace ${id}`,
    directory: `/workspace/${id}`,
    activePaneId: id,
    root,
  };
}

function splitRatio(node: PaneNodeSnapshot | undefined): number | undefined {
  return node?.type === "split" ? node.ratio : undefined;
}

describe("workspace snapshot state", () => {
  test("selects the active workspace", () => {
    const activeWorkspace = workspace(2);
    const snapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: 2,
      workspaces: [workspace(1), activeWorkspace],
    };

    expect(activeWorkspaceFrom(snapshot)).toBe(activeWorkspace);
    expect(activeWorkspaceFrom({ ...snapshot, activeWorkspaceId: 3 })).toBeNull();
    expect(activeWorkspaceFrom(null)).toBeNull();
  });

  test("selects the adjacent workspace when closing the active one", () => {
    const first = workspace(1);
    const second = workspace(2);
    const third = workspace(3);
    const snapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: second.id,
      workspaces: [first, second, third],
    };

    expect(closeWorkspaceLocally(snapshot, second.id)).toEqual({
      activeWorkspaceId: third.id,
      workspaces: [first, third],
    });
    expect(
      closeWorkspaceLocally({ activeWorkspaceId: third.id, workspaces: [first, third] }, third.id),
    ).toEqual({ activeWorkspaceId: first.id, workspaces: [first] });
  });

  test("preserves identity when closing an unknown workspace", () => {
    const snapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: 1,
      workspaces: [workspace(1)],
    };

    expect(closeWorkspaceLocally(snapshot, 99)).toBe(snapshot);
  });

  test("moves a workspace without mutating the snapshot or changing the active workspace", () => {
    const first = workspace(1);
    const second = workspace(2);
    const third = workspace(3);
    const snapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: third.id,
      workspaces: [first, second, third],
    };

    expect(moveWorkspaceLocally(snapshot, first.id, 3)).toEqual({
      activeWorkspaceId: third.id,
      workspaces: [second, third, first],
    });
    expect(snapshot.workspaces).toEqual([first, second, third]);
  });

  test("preserves identity for invalid and unchanged workspace moves", () => {
    const snapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: 1,
      workspaces: [workspace(1), workspace(2)],
    };

    expect(moveWorkspaceLocally(snapshot, 99, 0)).toBe(snapshot);
    expect(moveWorkspaceLocally(snapshot, 1, 1)).toBe(snapshot);
    expect(moveWorkspaceLocally(snapshot, 1, -1)).toBe(snapshot);
  });

  test("resizes a matching split without mutating the snapshot", () => {
    const root = split(10, 0.5);
    const snapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: 1,
      workspaces: [workspace(1, root)],
    };

    const resized = resizeSplitLocally(snapshot, 1, 10, 0.7);

    expect(resized).not.toBe(snapshot);
    expect(splitRatio(resized?.workspaces[0]?.root)).toBe(0.7);
    expect(snapshot.workspaces[0]?.root).toBe(root);
    expect(resizeSplitLocally(snapshot, 1, 10, 1)).toBe(snapshot);
    expect(resizeSplitLocally(snapshot, 1, 99, 0.7)).toBe(snapshot);
  });

  test("merges local split ratios into a fresh server snapshot", () => {
    const serverWorkspace = workspace(1, split(10, 0.5));
    const localWorkspace = workspace(1, split(10, 0.8));
    const serverSnapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: 1,
      workspaces: [serverWorkspace],
    };
    const localSnapshot: WorkspaceListSnapshot = {
      activeWorkspaceId: 1,
      workspaces: [localWorkspace],
    };

    const merged = mergeLocalSplitRatios(serverSnapshot, localSnapshot);

    expect(splitRatio(merged.workspaces[0]?.root)).toBe(0.8);
    expect(serverWorkspace.root).toEqual(split(10, 0.5));
    expect(mergeLocalSplitRatios(serverSnapshot, null)).toBe(serverSnapshot);
  });

});
