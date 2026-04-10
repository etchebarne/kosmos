import { useEffect, useRef } from "react";
import { NodeResizer, type NodeProps, useReactFlow } from "@xyflow/react";
import type { InfinityNode as InfinityNodeType } from "../../store/infinity.store";
import { useInfinityStore } from "../../store/infinity.store";
import { useLayoutStore } from "../../store/layout.store";
import { getTabDefinition } from "../registry";
import { TabIcon } from "../../components/shared/TabIcon";

export function InfinityNode({ id, data }: NodeProps<InfinityNodeType>) {
  const removeNode = useInfinityStore((s) => s.removeNode);
  const isDirty = useLayoutStore((s) => s.dirtyTabs.has(`infinity-${id}`));
  const contentRef = useRef<HTMLDivElement>(null);
  const reactFlow = useReactFlow();
  const definition = getTabDefinition(data.tabType);
  if (!definition) return null;

  const Component = definition.component;
  const pseudoTab = {
    id: `infinity-${id}`,
    type: data.tabType,
    title: data.title,
    icon: data.icon,
    ...(data.metadata && { metadata: data.metadata }),
  };

  // Native wheel listeners (React synthetic stopPropagation doesn't block d3-zoom).
  // Capture phase: intercept ctrl+wheel before children (e.g. xterm) consume it,
  // then re-dispatch from the parent so it still bubbles up to ReactFlow for zoom.
  // Bubble phase: stop normal wheel from reaching ReactFlow so inner ScrollAreas scroll.
  // eslint-disable-next-line react-hooks/rules-of-hooks
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;
    const captureHandler = (e: WheelEvent) => {
      if (e.ctrlKey) {
        e.stopPropagation();
        el.parentElement?.dispatchEvent(new WheelEvent(e.type, e));
      }
    };
    const bubbleHandler = (e: WheelEvent) => {
      if (!e.ctrlKey) e.stopPropagation();
    };
    el.addEventListener("wheel", captureHandler, { capture: true });
    el.addEventListener("wheel", bubbleHandler, { passive: true });
    return () => {
      el.removeEventListener("wheel", captureHandler, { capture: true });
      el.removeEventListener("wheel", bubbleHandler);
    };
  }, []);

  // Adjust mouse/pointer coordinates for CSS-scaled content.
  // ReactFlow applies transform: scale(z) to the viewport; child components
  // (e.g. xterm.js) that derive positions from clientX/clientY paired with
  // getBoundingClientRect() see a scaled rect but expect unscaled offsets,
  // causing clicks and selections to land at the wrong position.
  // eslint-disable-next-line react-hooks/rules-of-hooks
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;

    const adjust = (e: MouseEvent) => {
      // Skip adjustment for middle-button (pan) events — ReactFlow's d3-zoom
      // needs raw screen coordinates; adjusting them causes a jump on pan start.
      if (e.button === 1 || e.buttons === 4) return;
      const z = reactFlow.getZoom();
      if (z === 1) return;
      const rect = el.getBoundingClientRect();
      const ax = rect.left + (e.clientX - rect.left) / z;
      const ay = rect.top + (e.clientY - rect.top) / z;
      Object.defineProperty(e, "clientX", { value: ax, configurable: true });
      Object.defineProperty(e, "clientY", { value: ay, configurable: true });
      Object.defineProperty(e, "x", { value: ax, configurable: true });
      Object.defineProperty(e, "y", { value: ay, configurable: true });
    };

    const events = [
      "pointerdown",
      "pointermove",
      "pointerup",
      "mousedown",
      "mousemove",
      "mouseup",
    ] as const;
    for (const evt of events) {
      el.addEventListener(evt, adjust as EventListener, { capture: true });
    }
    return () => {
      for (const evt of events) {
        el.removeEventListener(evt, adjust as EventListener, { capture: true });
      }
    };
  }, [reactFlow]);

  return (
    <>
      <NodeResizer
        minWidth={200}
        minHeight={160}
        handleStyle={{
          width: 12,
          height: 12,
          borderRadius: 0,
          backgroundColor: "transparent",
          border: "none",
        }}
        lineStyle={{
          borderColor: "transparent",
          borderWidth: 8,
        }}
      />
      <div className="flex flex-col h-full w-full bg-[var(--color-bg-page)] border border-[var(--color-border-primary)] rounded-lg shadow-lg overflow-hidden">
        {/* Title bar — drag handle */}
        <div className="infinity-node-handle flex items-center gap-2 px-3 h-9 shrink-0 bg-[var(--color-bg-surface)] border-b border-[var(--color-border-secondary)] cursor-grab select-none">
          <TabIcon
            name={data.icon}
            size={14}
            className="shrink-0 text-[var(--color-text-tertiary)]"
          />
          <span className="text-xs text-[var(--color-text-secondary)] truncate flex-1">
            {data.title}
            {isDirty && (
              <span
                className="inline-block w-2 h-2 ml-1.5 align-middle bg-[var(--color-text-primary)]"
                style={{ borderRadius: "50%" }}
              />
            )}
          </span>
          <button
            className="flex items-center justify-center w-5 h-5 rounded hover:bg-[var(--color-bg-hover)] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            onClick={() => removeNode(data.tabId, id)}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <svg
              width="10"
              height="10"
              viewBox="0 0 8 8"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
            >
              <path d="M1 1L7 7M7 1L1 7" />
            </svg>
          </button>
        </div>

        {/* Content — nodrag to prevent drag */}
        <div ref={contentRef} className="flex-1 overflow-hidden nodrag">
          <Component tab={pseudoTab} paneId={`infinity-${id}`} />
        </div>
      </div>
    </>
  );
}
