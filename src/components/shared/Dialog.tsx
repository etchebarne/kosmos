import { useRef, useEffect, useState, type ReactNode } from "react";

interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
}

export function Dialog({ open, onClose, title, children }: DialogProps) {
  const [shouldRender, setShouldRender] = useState(open);
  const isClosing = shouldRender && !open;
  const overlayRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) setShouldRender(true);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open, onClose]);

  const handleAnimationEnd = () => {
    if (!open) setShouldRender(false);
  };

  if (!shouldRender) return null;

  return (
    <div
      ref={overlayRef}
      className={`fixed inset-0 z-50 flex items-center justify-center bg-black/50 ${isClosing ? "animate-fade-out" : "animate-fade-in"}`}
      onAnimationEnd={handleAnimationEnd}
      onMouseDown={(e) => {
        if (e.target === overlayRef.current) onClose();
      }}
    >
      <div
        className={`w-full max-w-md bg-[var(--color-bg-page)] border border-[var(--color-border-primary)] shadow-[6px_6px_0_rgba(0,0,0,0.25)] flex flex-col max-h-[70vh] rounded-xl overflow-hidden ${isClosing ? "animate-fade-out-down" : "animate-fade-in-up"}`}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--color-border-primary)]">
          <span className="text-sm font-medium text-[var(--color-text-primary)]">{title}</span>
          <button
            className="w-5 h-5 flex items-center justify-center text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-hover)] transition-colors cursor-pointer rounded-md"
            onClick={onClose}
          >
            &times;
          </button>
        </div>
        <div className="flex-1 overflow-auto">{children}</div>
      </div>
    </div>
  );
}
