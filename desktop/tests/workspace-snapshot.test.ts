import { describe, expect, test } from "bun:test";

import {
  activePaneOf,
  activeWorkspaceFrom,
  adjacentTabId,
  findTabOfKind,
  closeWorkspaceLocally,
  mergeLocalSplitRatios,
  moveWorkspaceLocally,
  resizeSplitLocally,
} from "@/renderer/lib/workspace-snapshot";
import type {
  PaneNodeSnapshot,
  TabKind,
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

describe("pane tab navigation", () => {
  const pane = (id: number, activeTabId: number, kinds: TabKind[]) => ({
    id,
    activeTabId,
    tabs: kinds.map((kind, index) => ({
      id: id * 100 + index,
      kind,
      lifecycle: "keepAlive" as const,
      title: kind,
    })),
  });
  const workspace = (activePaneId: number): WorkspaceSnapshot => ({
    id: 1,
    name: "kosmos",
    directory: "/kosmos",
    activePaneId,
    root: {
      type: "split",
      id: 9,
      axis: "horizontal",
      ratio: 0.5,
      first: { type: "leaf", pane: pane(1, 100, ["terminal", "search", "git"]) },
      second: { type: "leaf", pane: pane(2, 201, ["search", "editor"]) },
    },
  });

  test("cycles tabs and wraps around", () => {
    const first = pane(1, 100, ["terminal", "search", "git"]);
    expect(adjacentTabId(first, 1)).toBe(101);
    expect(adjacentTabId(first, -1)).toBe(102);
    expect(adjacentTabId({ ...first, activeTabId: 102 }, 1)).toBe(100);
    expect(adjacentTabId(pane(3, 300, ["terminal"]), 1)).toBeNull();
  });

  test("resolves the active pane", () => {
    expect(activePaneOf(workspace(2))?.id).toBe(2);
    expect(activePaneOf(workspace(7))).toBeNull();
  });

  test("prefers a tab of the requested kind in the active pane", () => {
    expect(findTabOfKind(workspace(2), "search")).toEqual({ paneId: 2, tabId: 200 });
    expect(findTabOfKind(workspace(1), "search")).toEqual({ paneId: 1, tabId: 101 });
    expect(findTabOfKind(workspace(2), "git")).toEqual({ paneId: 1, tabId: 102 });
    expect(findTabOfKind(workspace(1), "fileTree")).toBeNull();
  });
});
