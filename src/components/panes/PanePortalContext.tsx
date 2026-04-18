import { createContext, useContext, useRef, useEffect, useLayoutEffect, useMemo } from "react";
import { createPortal } from "react-dom";
import { findAllLeaves } from "../../lib/paneTree";
import { TabContent } from "./TabContent";
import type { PaneNode, Tab } from "../../types";

/**
 * Manages stable DOM containers for each tab.
 * Each tab gets a persistent <div> that follows it across pane moves.
 * Portal content renders into these stable divs, so React never unmounts
 * the tab components — even when a tab is dragged to a different pane.
 */
class PaneContainerRegistry {
  private tabContainers = new Map<string, HTMLDivElement>();
  private scrollSnapshots = new Map<string, { el: Element; top: number; left: number }[]>();

  getTab(tabId: string): HTMLDivElement {
    let el = this.tabContainers.get(tabId);
    if (!el) {
      el = document.createElement("div");
      el.style.height = "100%";
      this.tabContainers.set(tabId, el);
    }
    return el;
  }

  /** Capture scroll positions while the container is still in the live DOM. */
  saveScroll(tabId: string) {
    const container = this.tabContainers.get(tabId);
    if (!container || !container.isConnected) return;
    const saved: { el: Element; top: number; left: number }[] = [];
    container.querySelectorAll("*").forEach((el) => {
      if (el.scrollTop || el.scrollLeft) {
        saved.push({ el, top: el.scrollTop, left: el.scrollLeft });
      }
    });
    this.scrollSnapshots.set(tabId, saved);
  }

  /** Restore previously captured scroll positions. */
  restoreScroll(tabId: string) {
    const saved = this.scrollSnapshots.get(tabId);
    if (!saved) return;
    for (const { el, top, left } of saved) {
      el.scrollTop = top;
      el.scrollLeft = left;
    }
    this.scrollSnapshots.delete(tabId);
  }

  cleanup(validIds: Set<string>) {
    for (const [id, el] of this.tabContainers) {
      if (!validIds.has(id)) {
        el.remove();
        this.tabContainers.delete(id);
        this.scrollSnapshots.delete(id);
      }
    }
  }
}

const RegistryContext = createContext<PaneContainerRegistry>(null!);

/** Adopt the stable tab containers for this pane's tabs. */
export function usePaneContainer(
  _paneId: string,
  tabs: Tab[],
  activeTabId: string | null | undefined,
  contentRef: React.RefObject<HTMLDivElement | null>,
) {
  const registry = useContext(RegistryContext);

  // Save scrolls during render (before React reparents) while old parents still exist.
  for (const tab of tabs) {
    registry.saveScroll(tab.id);
  }

  // appendChild moves containers (not clones), preserving mounted state across panes.
  useLayoutEffect(() => {
    const div = contentRef.current;
    if (!div) return;

    const resolvedActiveId = activeTabId ?? tabs[0]?.id;

    for (const tab of tabs) {
      const tabContainer = registry.getTab(tab.id);

      if (tabContainer.parentElement !== div) {
        div.appendChild(tabContainer);
        // xterm (and similar canvas renderers) needs a repaint after a DOM move.
        tabContainer.dispatchEvent(new Event("pane-changed", { bubbles: true }));
      }

      if (tab.id === resolvedActiveId) {
        tabContainer.className = "h-full";
        tabContainer.removeAttribute("inert");
      } else {
        tabContainer.className = "opacity-0 absolute inset-0 overflow-hidden pointer-events-none";
        tabContainer.setAttribute("inert", "");
      }
    }

    for (const tab of tabs) {
      registry.restoreScroll(tab.id);
    }
  });
}

/** Renders each tab into a persistent DOM container so pane moves don't remount. */
export function PanePortalProvider({
  layout,
  children,
}: {
  layout: PaneNode;
  children: React.ReactNode;
}) {
  const registryRef = useRef<PaneContainerRegistry>(null!);
  if (!registryRef.current) {
    registryRef.current = new PaneContainerRegistry();
  }
  const registry = registryRef.current;

  const allLeaves = findAllLeaves(layout);

  const allTabIds = useMemo(
    () => new Set(allLeaves.flatMap((l) => l.tabs.map((t) => t.id))),
    [allLeaves],
  );

  useEffect(() => {
    registry.cleanup(allTabIds);
  }, [registry, allTabIds]);

  return (
    <RegistryContext.Provider value={registry}>
      {children}
      {allLeaves.flatMap((leaf) =>
        // Sort by id to keep React reconciliation order stable across re-renders.
        [...leaf.tabs]
          .sort((a, b) => a.id.localeCompare(b.id))
          .map((tab) =>
            createPortal(
              <TabContent tab={tab} paneId={leaf.id} />,
              registry.getTab(tab.id),
              tab.id,
            ),
          ),
      )}
    </RegistryContext.Provider>
  );
}
