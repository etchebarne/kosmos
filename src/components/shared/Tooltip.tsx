import { useState, useRef, useLayoutEffect } from "react";
import { createPortal } from "react-dom";

interface TooltipProps {
  content: React.ReactNode;
  children: React.ReactElement;
  delay?: number;
  side?: "top" | "bottom";
}

export function Tooltip({ content, children, delay = 400, side = "bottom" }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [position, setPosition] = useState({ x: 0, y: 0, actualSide: side });
  const triggerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleHover = (hovering: boolean) => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
    if (hovering) {
      timeoutRef.current = setTimeout(() => setVisible(true), delay);
    } else {
      setVisible(false);
    }
  };

  useLayoutEffect(() => {
    if (!visible || !triggerRef.current || !tooltipRef.current) return;

    const triggerRect = triggerRef.current.getBoundingClientRect();
    const tooltipRect = tooltipRef.current.getBoundingClientRect();
    const padding = 6;

    let x = triggerRect.left + triggerRect.width / 2;
    let actualSide = side;

    if (
      side === "bottom" &&
      triggerRect.bottom + padding + tooltipRect.height > window.innerHeight
    ) {
      actualSide = "top";
    } else if (side === "top" && triggerRect.top - padding - tooltipRect.height < 0) {
      actualSide = "bottom";
    }

    const y = actualSide === "bottom" ? triggerRect.bottom + padding : triggerRect.top - padding;

    const halfWidth = tooltipRect.width / 2;
    if (x - halfWidth < padding) {
      x = halfWidth + padding;
    } else if (x + halfWidth > window.innerWidth - padding) {
      x = window.innerWidth - halfWidth - padding;
    }

    setPosition({ x, y, actualSide });
  }, [visible, side]);

  return (
    <>
      <div
        ref={triggerRef}
        onMouseEnter={() => handleHover(true)}
        onMouseLeave={() => handleHover(false)}
        className="inline-flex"
      >
        {children}
      </div>
      {visible &&
        createPortal(
          <div
            className="fixed z-50 pointer-events-none"
            style={{
              left: position.x,
              top: position.y,
              transform:
                position.actualSide === "bottom"
                  ? "translateX(-50%)"
                  : "translateX(-50%) translateY(-100%)",
            }}
          >
            <div
              ref={tooltipRef}
              className={`px-2.5 py-1.5 text-xs font-medium tracking-wide whitespace-nowrap bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] text-[var(--color-text-primary)] shadow-[3px_3px_0_rgba(0,0,0,0.25)] rounded-sm ${
                position.actualSide === "bottom" ? "animate-fade-in-up" : "animate-fade-in-down"
              }`}
            >
              {content}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
