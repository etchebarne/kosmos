import { useEffect } from "react";

import {
  appShortcutForEvent,
  isTerminalOwnedShortcut,
  type AppShortcut,
} from "@/renderer/lib/app-shortcuts";
import {
  activePaneOf,
  activeWorkspaceFrom,
  adjacentTabId,
  findTabOfKind,
} from "@/renderer/lib/workspace-snapshot";
import { useQuickOpenStore } from "@/renderer/stores/quick-open-store";
import { useWorkspaceStore } from "@/renderer/stores/workspace-store";

type WorkspaceShortcut = Exclude<AppShortcut, "workspaceSymbols">;

/** Installs app-level tab shortcuts that work regardless of which view has focus. */
export function AppShortcutListener() {
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      const shortcut = appShortcutForEvent(event);
      if (!shortcut || shortcut === "workspaceSymbols" || isModalDialogOpen()) {
        return;
      }
      if (isTerminalOwnedShortcut(shortcut) && isInsideTerminal(event.target)) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      runWorkspaceShortcut(shortcut);
    };
    window.addEventListener("keydown", listener, true);
    return () => window.removeEventListener("keydown", listener, true);
  }, []);

  return null;
}

function runWorkspaceShortcut(shortcut: WorkspaceShortcut): void {
  switch (shortcut) {
    case "closeTab":
      closeActiveTab();
      return;
    case "nextTab":
      cycleActiveTab(1);
      return;
    case "previousTab":
      cycleActiveTab(-1);
      return;
    case "quickOpen":
      quickOpen();
      return;
  }
}

function closeActiveTab(): void {
  const pane = activePane();
  if (pane) {
    useWorkspaceStore.getState().closeTab(pane.id, pane.activeTabId);
  }
}

function cycleActiveTab(offset: number): void {
  const pane = activePane();
  const tabId = pane ? adjacentTabId(pane, offset) : null;
  if (pane && tabId !== null) {
    useWorkspaceStore.getState().activateTab(pane.id, tabId);
  }
}

function quickOpen(): void {
  const store = useWorkspaceStore.getState();
  const workspace = activeWorkspaceFrom(store.snapshot);
  if (!workspace) {
    return;
  }

  const searchTab = findTabOfKind(workspace, "search");
  if (!searchTab) {
    const pane = activePaneOf(workspace);
    if (pane) {
      store.openTab(pane.id, "search");
    }
    return;
  }

  store.activateTab(searchTab.paneId, searchTab.tabId);
  useQuickOpenStore.getState().requestQuickOpen(workspace.id, searchTab.tabId);
}

function activePane() {
  const workspace = activeWorkspaceFrom(useWorkspaceStore.getState().snapshot);
  return workspace ? activePaneOf(workspace) : null;
}

/** App dialogs render `DialogContent`; Monaco's find widget also uses role="dialog", so match the slot. */
function isModalDialogOpen(): boolean {
  return document.querySelector('[data-slot="dialog-content"]:not([data-closed])') !== null;
}

function isInsideTerminal(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(".xterm") !== null;
}
