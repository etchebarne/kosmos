import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { PaneNode, PaneSplit, Tab } from "../types";
import {
  genId,
  createTab,
  createLeaf,
  findLeaf,
  findAllLeaves,
  updateNode,
} from "../lib/pane-tree";
import "../tabs"; // Initialize tab registry
import { getTabDefinition } from "../tabs";
import { useDragStore } from "./drag.store";

interface LayoutStore {
  layout: PaneNode;
  layouts: Record<string, PaneNode>;
  activeWorkspacePath: string | null;
  lastEditorPaneId: string | null;
  activePaneId: string | null;

  setWorkspace: (path: string | null) => void;
  addTab: (
    paneId: string,
    type?: string,
    title?: string,
    metadata?: Record<string, unknown>,
  ) => void;
  closeTab: (paneId: string, tabId: string) => void;
  closeOtherTabs: (paneId: string, tabId: string) => void;
  closeTabsToLeft: (paneId: string, tabId: string) => void;
  closeTabsToRight: (paneId: string, tabId: string) => void;
  closeAllTabs: (paneId: string) => void;
  setActiveTab: (paneId: string, tabId: string) => void;
  reorderTab: (paneId: string, fromIndex: number, toIndex: number) => void;
  moveTabToPane: (fromPaneId: string, tabId: string, toPaneId: string, index?: number) => void;
  transformTab: (paneId: string, tabId: string, newType: string) => void;
  splitPane: (
    targetPaneId: string,
    direction: "horizontal" | "vertical",
    tab: Tab,
    sourcePaneId: string,
    position: "before" | "after",
  ) => void;
  setPaneSizes: (splitId: string, sizes: number[]) => void;
  insertSplit: (
    targetPaneId: string,
    direction: "horizontal" | "vertical",
    position: "before" | "after",
    type: string,
    title?: string,
    metadata?: Record<string, unknown>,
  ) => void;
  openFile: (filePath: string, fileName: string, sourcePaneId: string) => void;
  openChanges: (
    filePath: string,
    fileName: string,
    staged: boolean,
    isUntracked: boolean,
    sourcePaneId: string,
  ) => void;
  openSearch: () => void;
  dirtyTabs: Set<string>;
  setTabDirty: (tabId: string, dirty: boolean) => void;
}

function makeSplit(
  direction: "horizontal" | "vertical",
  children: [PaneNode, PaneNode],
): PaneSplit {
  return { id: genId(), type: "split", direction, children, sizes: [50, 50] };
}

/**
 * Shared logic for opening a tab in the best available pane.
 * Used by both openFile and openChanges to avoid duplication.
 */
function openTabInBestPane(
  state: { layout: PaneNode; lastEditorPaneId: string | null },
  tabType: string,
  filePath: string,
  tabTitle: string,
  metadata: Record<string, unknown>,
  sourcePaneId: string,
) {
  const leaves = findAllLeaves(state.layout);

  // Check if a tab for this file is already open
  for (const leaf of leaves) {
    const existing = leaf.tabs.find(
      (t) => t.type === tabType && (t.metadata?.filePath as string) === filePath,
    );
    if (existing) {
      const layout =
        updateNode(state.layout, leaf.id, (l) => ({
          ...l,
          activeTabId: existing.id,
        })) ?? state.layout;
      return { layout, lastEditorPaneId: leaf.id, activePaneId: leaf.id };
    }
  }

  // Find the best pane to open a new tab in
  const editorPanes = leaves.filter((leaf) =>
    leaf.tabs.some((t) => t.type === "editor" || t.type === "changes" || t.type === "infinity"),
  );

  let targetPaneId: string | null = null;
  if (editorPanes.length === 1) {
    targetPaneId = editorPanes[0].id;
  } else if (editorPanes.length > 1) {
    const lastUsed = editorPanes.find((p) => p.id === state.lastEditorPaneId);
    targetPaneId = lastUsed ? lastUsed.id : editorPanes[0].id;
  }

  const tab = createTab(tabType, tabTitle, metadata);

  if (targetPaneId) {
    const layout =
      updateNode(state.layout, targetPaneId, (leaf) => ({
        ...leaf,
        tabs: [...leaf.tabs, tab],
        activeTabId: tab.id,
      })) ?? state.layout;
    return { layout, lastEditorPaneId: targetPaneId, activePaneId: targetPaneId };
  }

  const newLeaf = createLeaf([tab]);
  const layout =
    updateNode(state.layout, sourcePaneId, (leaf) => {
      return makeSplit("horizontal", [leaf, newLeaf]);
    }) ?? state.layout;

  return { layout, lastEditorPaneId: newLeaf.id, activePaneId: newLeaf.id };
}

export const useLayoutStore = create<LayoutStore>((set) => ({
  layout: createLeaf(),
  layouts: {},
  activeWorkspacePath: null,
  lastEditorPaneId: null,
  activePaneId: null,
  dirtyTabs: new Set<string>(),

  setWorkspace: (path) =>
    set((state) => {
      // Save current layout
      const layouts = { ...state.layouts };
      if (state.activeWorkspacePath) {
        layouts[state.activeWorkspacePath] = state.layout;
      }

      // Load or create layout for new workspace
      const layout = path ? (layouts[path] ?? createLeaf()) : createLeaf();
      useDragStore.getState().setDragState(null);
      return { layouts, layout, activeWorkspacePath: path };
    }),

  addTab: (paneId, type = "blank", title, metadata) =>
    set((state) => {
      const tab = createTab(type, title, metadata);
      const layout = updateNode(state.layout, paneId, (leaf) => ({
        ...leaf,
        tabs: [...leaf.tabs, tab],
        activeTabId: tab.id,
      }));
      return { layout: layout ?? createLeaf() };
    }),

  transformTab: (paneId, tabId, newType) =>
    set((state) => {
      const definition = getTabDefinition(newType);
      if (!definition) return state;
      const layout = updateNode(state.layout, paneId, (leaf) => ({
        ...leaf,
        tabs: leaf.tabs.map((t) =>
          t.id === tabId
            ? { ...t, type: newType, title: definition.title, icon: definition.icon }
            : t,
        ),
      }));
      return { layout: layout ?? state.layout };
    }),

  closeTab: (paneId, tabId) =>
    set((state) => {
      const layout = updateNode(state.layout, paneId, (leaf) => {
        const tabs = leaf.tabs.filter((t) => t.id !== tabId);
        if (tabs.length === 0) return null;
        const activeTabId =
          leaf.activeTabId === tabId
            ? (tabs[
                Math.min(
                  leaf.tabs.findIndex((t) => t.id === tabId),
                  tabs.length - 1,
                )
              ]?.id ?? null)
            : leaf.activeTabId;
        return { ...leaf, tabs, activeTabId };
      });
      return { layout: layout ?? createLeaf() };
    }),

  closeOtherTabs: (paneId, tabId) =>
    set((state) => {
      const layout = updateNode(state.layout, paneId, (leaf) => {
        const tabs = leaf.tabs.filter((t) => t.id === tabId);
        return { ...leaf, tabs, activeTabId: tabId };
      });
      return { layout: layout ?? createLeaf() };
    }),

  closeTabsToLeft: (paneId, tabId) =>
    set((state) => {
      const layout = updateNode(state.layout, paneId, (leaf) => {
        const idx = leaf.tabs.findIndex((t) => t.id === tabId);
        const tabs = leaf.tabs.slice(idx);
        const activeTabId = tabs.some((t) => t.id === leaf.activeTabId) ? leaf.activeTabId : tabId;
        return { ...leaf, tabs, activeTabId };
      });
      return { layout: layout ?? createLeaf() };
    }),

  closeTabsToRight: (paneId, tabId) =>
    set((state) => {
      const layout = updateNode(state.layout, paneId, (leaf) => {
        const idx = leaf.tabs.findIndex((t) => t.id === tabId);
        const tabs = leaf.tabs.slice(0, idx + 1);
        const activeTabId = tabs.some((t) => t.id === leaf.activeTabId) ? leaf.activeTabId : tabId;
        return { ...leaf, tabs, activeTabId };
      });
      return { layout: layout ?? createLeaf() };
    }),

  closeAllTabs: (paneId) =>
    set((state) => {
      const layout = updateNode(state.layout, paneId, () => null);
      return { layout: layout ?? createLeaf() };
    }),

  setActiveTab: (paneId, tabId) =>
    set((state) => ({
      activePaneId: paneId,
      layout:
        updateNode(state.layout, paneId, (leaf) => ({
          ...leaf,
          activeTabId: tabId,
        })) ?? state.layout,
    })),

  reorderTab: (paneId, fromIndex, toIndex) =>
    set((state) => ({
      layout:
        updateNode(state.layout, paneId, (leaf) => {
          const tabs = [...leaf.tabs];
          const [moved] = tabs.splice(fromIndex, 1);
          tabs.splice(toIndex, 0, moved);
          return { ...leaf, tabs };
        }) ?? state.layout,
    })),

  moveTabToPane: (fromPaneId, tabId, toPaneId, index) =>
    set((state) => {
      const sourceLeaf = findLeaf(state.layout, fromPaneId);
      if (!sourceLeaf) return state;
      const tab = sourceLeaf.tabs.find((t) => t.id === tabId);
      if (!tab) return state;

      let layout = updateNode(state.layout, fromPaneId, (leaf) => {
        const tabs = leaf.tabs.filter((t) => t.id !== tabId);
        if (tabs.length === 0) return null;
        return {
          ...leaf,
          tabs,
          activeTabId: leaf.activeTabId === tabId ? (tabs[0]?.id ?? null) : leaf.activeTabId,
        };
      });
      if (!layout) layout = createLeaf();

      layout =
        updateNode(layout, toPaneId, (leaf) => {
          const tabs = [...leaf.tabs];
          const insertAt = index ?? tabs.length;
          tabs.splice(insertAt, 0, tab);
          return { ...leaf, tabs, activeTabId: tab.id };
        }) ?? layout;

      return { layout };
    }),

  splitPane: (targetPaneId, direction, tab, sourcePaneId, position) =>
    set((state) => {
      const sourceLeaf = findLeaf(state.layout, sourcePaneId);
      if (sourcePaneId === targetPaneId && sourceLeaf && sourceLeaf.tabs.length <= 1) {
        return state;
      }

      let layout = updateNode(state.layout, sourcePaneId, (leaf) => {
        const tabs = leaf.tabs.filter((t) => t.id !== tab.id);
        if (tabs.length === 0 && sourcePaneId !== targetPaneId) return null;
        if (tabs.length === 0) return leaf;
        return {
          ...leaf,
          tabs,
          activeTabId: leaf.activeTabId === tab.id ? (tabs[0]?.id ?? null) : leaf.activeTabId,
        };
      });
      if (!layout) layout = createLeaf();

      const newLeaf = createLeaf([tab]);
      layout =
        updateNode(layout, targetPaneId, (leaf) => {
          const children: [PaneNode, PaneNode] =
            position === "before" ? [newLeaf, leaf] : [leaf, newLeaf];
          return makeSplit(direction, children);
        }) ?? layout;

      return { layout };
    }),

  setPaneSizes: (splitId, sizes) =>
    set((state) => {
      function update(node: PaneNode): PaneNode {
        if (node.type === "leaf") return node;
        if (node.id === splitId) return { ...node, sizes };
        return { ...node, children: node.children.map(update) };
      }
      return { layout: update(state.layout) };
    }),

  insertSplit: (targetPaneId, direction, position, type, title, metadata) =>
    set((state) => {
      const tab = createTab(type, title, metadata);
      const newLeaf = createLeaf([tab]);
      const layout =
        updateNode(state.layout, targetPaneId, (leaf) => {
          const children: [PaneNode, PaneNode] =
            position === "before" ? [newLeaf, leaf] : [leaf, newLeaf];
          return makeSplit(direction, children);
        }) ?? state.layout;
      return { layout };
    }),

  openFile: (filePath, fileName, sourcePaneId) => {
    // Fire-and-forget frecency update so fff boosts this file next time.
    invoke("fff_track_access", { path: filePath }).catch(() => {});
    set((state) =>
      openTabInBestPane(state, "editor", filePath, fileName, { filePath }, sourcePaneId),
    );
  },

  openChanges: (filePath, fileName, staged, isUntracked, sourcePaneId) =>
    set((state) =>
      openTabInBestPane(
        state,
        "changes",
        filePath,
        fileName,
        { filePath, staged, isUntracked },
        sourcePaneId,
      ),
    ),

  openSearch: () =>
    set((state) => {
      const leaves = findAllLeaves(state.layout);

      // Find existing search tab
      for (const leaf of leaves) {
        const existing = leaf.tabs.find((t) => t.type === "search");
        if (existing) {
          const layout =
            updateNode(state.layout, leaf.id, (l) => ({
              ...l,
              activeTabId: existing.id,
            })) ?? state.layout;
          return { layout, activePaneId: leaf.id };
        }
      }

      // No existing search tab — prefer a pane with editor/infinity tabs
      const primaryPanes = leaves.filter((leaf) =>
        leaf.tabs.some((t) => t.type === "editor" || t.type === "changes" || t.type === "infinity"),
      );
      let targetLeaf;
      if (primaryPanes.length === 1) {
        targetLeaf = primaryPanes[0];
      } else if (primaryPanes.length > 1) {
        const lastUsed = primaryPanes.find((p) => p.id === state.lastEditorPaneId);
        targetLeaf = lastUsed ?? primaryPanes[0];
      } else {
        targetLeaf = leaves[0];
      }
      if (!targetLeaf) return state;

      const tab = createTab("search", "Search");
      const layout =
        updateNode(state.layout, targetLeaf.id, (leaf) => ({
          ...leaf,
          tabs: [...leaf.tabs, tab],
          activeTabId: tab.id,
        })) ?? state.layout;
      return { layout, activePaneId: targetLeaf.id };
    }),

  setTabDirty: (tabId, dirty) =>
    set((state) => {
      const next = new Set(state.dirtyTabs);
      if (dirty) next.add(tabId);
      else next.delete(tabId);
      return { dirtyTabs: next };
    }),
}));
