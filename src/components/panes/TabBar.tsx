import { memo, useRef, useState, useCallback, useEffect } from "react";
import autoAnimate from "@formkit/auto-animate";
import { Plus, X } from "@phosphor-icons/react";
import { useLayoutStore } from "../../store/layout.store";
import { useDragStore } from "../../store/drag.store";
import { startDragThreshold } from "../../lib/drag-threshold";
import { TabIcon } from "../shared/TabIcon";
import { ContextMenu } from "../shared/ContextMenu";
import type { ContextMenuItem } from "../shared/ContextMenu";
import type { Tab } from "../../types";
import { getEditorMeta } from "../../types";
import { FileIcon } from "../../tabs/file-tree/file-icons";
import { useIsDarkTheme } from "../../lib/themes";

/** Split a file path into its basename and extension (matching the file-tree's shape). */
function parseFilePath(filePath: string): { name: string; extension: string | null } {
  const name = filePath.split(/[/\\]/).pop() ?? filePath;
  const dotIdx = name.lastIndexOf(".");
  const extension = dotIdx > 0 ? name.slice(dotIdx + 1) : null;
  return { name, extension };
}

/** Per-tab dirty indicator — subscribes only to its own tab's dirty state. */
const DirtyDot = memo(function DirtyDot({ tabId }: { tabId: string }) {
  const isDirty = useLayoutStore((s) => s.dirtyTabs.has(tabId));
  if (!isDirty) return null;
  return (
    <span
      className="absolute w-2 h-2 bg-[var(--color-text-primary)] group-hover:opacity-0 transition-opacity duration-100"
      style={{ borderRadius: "50%" }}
    />
  );
});

interface TabBarProps {
  paneId: string;
  tabs: Tab[];
  activeTabId: string | null;
}

export const TabBar = memo(function TabBar({ paneId, tabs, activeTabId }: TabBarProps) {
  const setActiveTab = useLayoutStore((s) => s.setActiveTab);
  const closeTab = useLayoutStore((s) => s.closeTab);
  const closeOtherTabs = useLayoutStore((s) => s.closeOtherTabs);
  const closeTabsToLeft = useLayoutStore((s) => s.closeTabsToLeft);
  const closeTabsToRight = useLayoutStore((s) => s.closeTabsToRight);
  const closeAllTabs = useLayoutStore((s) => s.closeAllTabs);
  const addTab = useLayoutStore((s) => s.addTab);
  const setDragState = useDragStore((s) => s.setDragState);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; tab: Tab } | null>(null);
  const isDark = useIsDarkTheme();

  const handleTabMouseDown = useCallback(
    (e: React.MouseEvent, tab: Tab) => {
      if (e.button !== 0) return;

      startDragThreshold(
        e.clientX,
        e.clientY,
        () => setDragState({ type: "tab", tab, sourcePaneId: paneId }),
        () => setActiveTab(paneId, tab.id),
      );
    },
    [paneId, setDragState, setActiveTab],
  );

  const tabBarRef = useRef<HTMLDivElement>(null);
  const prevTabCountRef = useRef(tabs.length);

  useEffect(() => {
    if (tabBarRef.current) {
      autoAnimate(tabBarRef.current, {
        duration: 150,
        easing: "cubic-bezier(0.16, 1, 0.3, 1)",
      });
    }
  }, []);

  useEffect(() => {
    if (tabs.length > prevTabCountRef.current && tabBarRef.current) {
      tabBarRef.current.scrollLeft = tabBarRef.current.scrollWidth;
    }
    prevTabCountRef.current = tabs.length;
  }, [tabs.length]);

  const handleWheel = useCallback((e: React.WheelEvent) => {
    if (tabBarRef.current && e.deltaY !== 0) {
      e.preventDefault();
      tabBarRef.current.scrollLeft += e.deltaY;
    }
  }, []);

  return (
    <div
      ref={tabBarRef}
      role="tablist"
      className="flex items-center h-9 min-h-9 bg-[var(--color-project-bar-bg)] border-b border-[var(--color-border-primary)] overflow-x-auto overflow-y-hidden [&::-webkit-scrollbar]:h-0"
      data-tabbar-pane={paneId}
      onWheel={handleWheel}
    >
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        const editorMeta = getEditorMeta(tab);
        const iconColorClass = isActive
          ? "text-[var(--color-accent-blue)]"
          : "text-[var(--color-text-tertiary)]";
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            data-tab
            className={`group flex items-center gap-2 h-full px-3 cursor-grab select-none whitespace-nowrap ${
              isActive
                ? "bg-[var(--color-tab-active-bg)] border-b-2 border-[var(--color-accent-blue)]"
                : "bg-[var(--color-tab-inactive-bg)] border-b border-[var(--color-border-primary)] hover:bg-[var(--color-bg-surface)]"
            }`}
            onMouseDown={(e) => handleTabMouseDown(e, tab)}
            onAuxClick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeTab(paneId, tab.id);
              }
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              setContextMenu({ x: e.clientX, y: e.clientY, tab });
            }}
          >
            {editorMeta ? (
              (() => {
                const { name, extension } = parseFilePath(editorMeta.filePath);
                return (
                  <FileIcon
                    name={name}
                    extension={extension}
                    size={14}
                    className={iconColorClass}
                    isDark={isDark}
                  />
                );
              })()
            ) : (
              <TabIcon name={tab.icon} size={14} className={`shrink-0 ${iconColorClass}`} />
            )}
            <span
              className={`text-xs ${isActive ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-secondary)]"}`}
            >
              {tab.title}
            </span>
            <div className="relative flex items-center justify-center w-4 h-4">
              <DirtyDot tabId={tab.id} />
              <button
                aria-label={`Close ${tab.title}`}
                className="flex items-center justify-center p-0.5 text-[var(--color-text-muted)] opacity-0 group-hover:opacity-100 transition-opacity duration-100 hover:text-[var(--color-text-primary)] hover:bg-[var(--color-border-secondary)]"
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(paneId, tab.id);
                }}
              >
                <X size={12} />
              </button>
            </div>
          </div>
        );
      })}
      <button
        className="flex items-center justify-center w-7 h-7 mx-1 text-[var(--color-text-muted)] shrink-0 hover:text-[var(--color-text-secondary)] hover:bg-[var(--color-border-primary)]"
        onClick={() => addTab(paneId)}
      >
        <Plus size={14} />
      </button>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={(() => {
            const tabIdx = tabs.findIndex((t) => t.id === contextMenu.tab.id);
            return [
              { label: "Close", onClick: () => closeTab(paneId, contextMenu.tab.id) },
              {
                label: "Close Others",
                onClick: () => closeOtherTabs(paneId, contextMenu.tab.id),
                disabled: tabs.length <= 1,
              },
              { separator: true },
              {
                label: "Close to the Left",
                onClick: () => closeTabsToLeft(paneId, contextMenu.tab.id),
                disabled: tabIdx === 0,
              },
              {
                label: "Close to the Right",
                onClick: () => closeTabsToRight(paneId, contextMenu.tab.id),
                disabled: tabIdx === tabs.length - 1,
              },
              { separator: true },
              { label: "Close All", onClick: () => closeAllTabs(paneId), destructive: true },
            ] as ContextMenuItem[];
          })()}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
});
