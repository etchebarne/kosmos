import { useEffect, useState, useRef } from "react";
import autoAnimate from "@formkit/auto-animate";
import { Info, Warning, XCircle, CheckCircle, type Icon } from "@phosphor-icons/react";
import { useToastStore, type Toast } from "../../store/toast.store";

const TYPE_ICONS: Record<Toast["type"], { Icon: Icon; color: string }> = {
  info: { Icon: Info, color: "var(--color-accent-blue)" },
  warning: { Icon: Warning, color: "rgb(251 146 60)" },
  error: { Icon: XCircle, color: "var(--color-status-red)" },
  success: { Icon: CheckCircle, color: "var(--color-status-green)" },
};

function ToastItem({ toast }: { toast: Toast }) {
  const removeToast = useToastStore((s) => s.removeToast);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    requestAnimationFrame(() => setVisible(true));

    if (toast.duration > 0) {
      const timer = setTimeout(() => {
        setVisible(false);
        setTimeout(() => removeToast(toast.id), 150);
      }, toast.duration);
      return () => clearTimeout(timer);
    }
  }, [toast.id, toast.duration, removeToast]);

  const dismiss = () => {
    setVisible(false);
    setTimeout(() => removeToast(toast.id), 150);
  };

  const { Icon: StatusIcon, color } = TYPE_ICONS[toast.type];

  return (
    <div
      className={`flex items-center gap-3 px-3 py-2.5 bg-[var(--color-bg-surface)] border border-[var(--color-border-primary)] shadow-[3px_3px_0_rgba(0,0,0,0.25)] rounded-xl overflow-hidden transition-all duration-150 ${visible ? "opacity-100 translate-x-0" : "opacity-0 translate-x-4"}`}
    >
      <StatusIcon size={16} weight="fill" color={color} className="shrink-0" />
      <span className="text-xs text-[var(--color-text-secondary)] flex-1">{toast.message}</span>
      {toast.action && (
        <button
          className="px-2 py-1 text-xs text-[var(--color-accent-blue)] hover:text-[var(--color-accent-blue-hover)] hover:bg-[var(--color-bg-hover)] transition-colors cursor-pointer whitespace-nowrap rounded-md"
          onClick={() => {
            toast.action!.onClick();
            dismiss();
          }}
        >
          {toast.action.label}
        </button>
      )}
      <button
        className="w-5 h-5 flex items-center justify-center text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-hover)] transition-colors cursor-pointer rounded-md"
        onClick={dismiss}
      >
        &times;
      </button>
    </div>
  );
}

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (containerRef.current) {
      autoAnimate(containerRef.current, {
        duration: 150,
        easing: "cubic-bezier(0.16, 1, 0.3, 1)",
      });
    }
  }, []);

  if (toasts.length === 0) return null;

  return (
    <div ref={containerRef} className="fixed bottom-8 right-3 z-50 flex flex-col gap-2 max-w-sm">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
}
