import { useRef, useState, useLayoutEffect } from "react";
import { createPortal } from "react-dom";
import { useClickOutside } from "../../hooks/useClickOutside";

export type ContextMenuItem =
  | {
      label: string;
      onClick: () => void;
      disabled?: boolean;
      destructive?: boolean;
      active?: boolean;
    }
  | { separator: true };

type ContextMenuPlacement = "top-start" | "top-end" | "bottom-start" | "bottom-end";

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
  /** Which corner of the menu the (x,y) refers to. Default: "top-start". */
  placement?: ContextMenuPlacement;
}

export function ContextMenu({ x, y, items, onClose, placement = "top-start" }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y, visible: false });

  useClickOutside(ref, onClose);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left = placement.endsWith("end") ? x - rect.width : x;
    let top = placement.startsWith("bottom") ? y - rect.height : y;
    if (left + rect.width > vw) left = vw - rect.width - 4;
    if (top + rect.height > vh) top = vh - rect.height - 4;
    if (left < 0) left = 4;
    if (top < 0) top = 4;
    setPos({ left, top, visible: true });
  }, [x, y, placement]);

  return createPortal(
    <div
      ref={ref}
      className="fixed z-50 min-w-[140px] py-1 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] rounded-md shadow-[3px_3px_0_rgba(0,0,0,0.25)] animate-fade-in-down"
      style={{ left: pos.left, top: pos.top, visibility: pos.visible ? "visible" : "hidden" }}
    >
      {items.map((item, i) =>
        "separator" in item ? (
          <div key={`sep-${i}`} className="my-1 border-t border-[var(--color-border-primary)]" />
        ) : (
          <button
            key={item.label}
            className={`w-full text-left px-3 py-1.5 text-xs ${
              item.disabled
                ? "text-[var(--color-text-muted)] cursor-default"
                : item.destructive
                  ? "text-[var(--color-status-red)] hover:bg-[var(--color-bg-input)]"
                  : item.active
                    ? "text-[var(--color-text-primary)] bg-[var(--color-bg-surface)] font-medium"
                    : "text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-input)] hover:text-[var(--color-text-primary)]"
            }`}
            onClick={() => {
              if (item.disabled) return;
              item.onClick();
              onClose();
            }}
          >
            {item.label}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
