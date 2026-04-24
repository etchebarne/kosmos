import { memo, useRef, useState, useCallback } from "react";
import type { PaneNode } from "../../types";
import { useLayoutStore } from "../../store/layout.store";
import { usePaneContainer } from "./PanePortalContext";
import { TabBar } from "./TabBar";
import { SharedPaneEditor } from "../../tabs/editor/SharedPaneEditor";

interface PaneContainerProps {
  node: PaneNode;
}

export const PaneContainer = memo(function PaneContainer({ node }: PaneContainerProps) {
  if (node.type === "split") {
    return <SplitView node={node} />;
  }
  return <LeafPane node={node} />;
});

function SplitView({ node }: { node: Extract<PaneNode, { type: "split" }> }) {
  const setPaneSizes = useLayoutStore((s) => s.setPaneSizes);
  const containerRef = useRef<HTMLDivElement>(null);
  const [resizing, setResizing] = useState<number | null>(null);

  const handleMouseDown = useCallback(
    (index: number) => (e: React.MouseEvent) => {
      e.preventDefault();
      setResizing(index);

      const container = containerRef.current;
      if (!container) return;

      const startPos = node.direction === "horizontal" ? e.clientX : e.clientY;
      const containerRect = container.getBoundingClientRect();
      const totalSize =
        node.direction === "horizontal" ? containerRect.width : containerRect.height;
      const startSizes = [...node.sizes];

      const onMouseMove = (ev: MouseEvent) => {
        const currentPos = node.direction === "horizontal" ? ev.clientX : ev.clientY;
        const delta = ((currentPos - startPos) / totalSize) * 100;
        const newSizes = [...startSizes];
        newSizes[index] = Math.max(10, startSizes[index] + delta);
        newSizes[index + 1] = Math.max(10, startSizes[index + 1] - delta);
        setPaneSizes(node.id, newSizes);
      };

      const onMouseUp = () => {
        setResizing(null);
        window.removeEventListener("mousemove", onMouseMove);
        window.removeEventListener("mouseup", onMouseUp);
      };

      window.addEventListener("mousemove", onMouseMove);
      window.addEventListener("mouseup", onMouseUp);
    },
    [node.direction, node.id, node.sizes, setPaneSizes],
  );

  return (
    <div
      ref={containerRef}
      className={`flex w-full h-full min-w-0 min-h-0 ${node.direction === "horizontal" ? "flex-row" : "flex-col"}`}
      style={{
        cursor:
          resizing !== null
            ? node.direction === "horizontal"
              ? "col-resize"
              : "row-resize"
            : undefined,
      }}
    >
      {node.children.map((child, i) => (
        <div key={child.id} style={{ display: "contents" }}>
          <div
            className="min-w-0 min-h-0 overflow-hidden flex"
            style={{
              [node.direction === "horizontal" ? "width" : "height"]: `${node.sizes[i]}%`,
            }}
          >
            <PaneContainer node={child} />
          </div>
          {i < node.children.length - 1 && (
            <div
              role="separator"
              aria-label={`Resize ${node.direction === "horizontal" ? "columns" : "rows"}`}
              className={`shrink-0 bg-[var(--color-divider)] z-10 relative hover:bg-[var(--color-accent-blue)] ${
                node.direction === "horizontal"
                  ? "w-px cursor-col-resize after:absolute after:inset-y-0 after:-left-[3px] after:-right-[3px]"
                  : "h-px cursor-row-resize after:absolute after:inset-x-0 after:-top-[3px] after:-bottom-[3px]"
              }`}
              onMouseDown={handleMouseDown(i)}
            />
          )}
        </div>
      ))}
    </div>
  );
}

function LeafPane({ node }: { node: Extract<PaneNode, { type: "leaf" }> }) {
  const contentRef = useRef<HTMLDivElement>(null);
  usePaneContainer(node.id, node.tabs, node.activeTabId, contentRef);

  const activePaneId = useLayoutStore((s) => s.activePaneId);
  const activeTab = node.tabs.find((t) => t.id === node.activeTabId) ?? null;

  return (
    <div className="flex flex-col w-full h-full min-w-0 min-h-0">
      <TabBar paneId={node.id} tabs={node.tabs} activeTabId={node.activeTabId} />
      <div
        role="tabpanel"
        data-pane-content={node.id}
        className="flex-1 min-h-0 bg-[var(--color-bg-page)] relative overflow-hidden"
      >
        <div ref={contentRef} className="absolute inset-0" />
        <SharedPaneEditor
          paneId={node.id}
          activeTab={activeTab}
          isPaneFocused={activePaneId === node.id}
        />
      </div>
    </div>
  );
}
