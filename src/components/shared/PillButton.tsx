import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";

type PillVariant = "default" | "accent" | "ghost" | "danger";
type PillSize = "sm" | "md";

interface PillButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  variant?: PillVariant;
  size?: PillSize;
  leadingIcon?: ReactNode;
  trailingIcon?: ReactNode;
  children?: ReactNode;
}

const SIZE_CLASSES: Record<PillSize, string> = {
  sm: "h-7 px-3 gap-1.5 text-[11px]",
  md: "h-9 px-4 gap-2 text-xs",
};

const VARIANT_CLASSES: Record<PillVariant, string> = {
  default:
    "bg-[var(--color-bg-page)] pill-depth border border-[var(--color-border-primary)] text-[var(--color-text-primary)] hover:border-[var(--color-border-secondary)]",
  ghost:
    "bg-transparent border border-transparent text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-surface)]",
  accent:
    "bg-[var(--color-accent-blue)] pill-depth border border-[var(--color-accent-blue)] text-white hover:bg-[var(--color-accent-blue-hover)] hover:border-[var(--color-accent-blue-hover)]",
  danger:
    "bg-[var(--color-bg-page)] pill-depth border border-[var(--color-border-primary)] text-[var(--color-status-red)] hover:border-[var(--color-status-red)]",
};

export const PillButton = forwardRef<HTMLButtonElement, PillButtonProps>(function PillButton(
  {
    variant = "default",
    size = "md",
    leadingIcon,
    trailingIcon,
    children,
    className = "",
    type = "button",
    ...rest
  },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={`inline-flex items-center justify-center rounded-full font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${SIZE_CLASSES[size]} ${VARIANT_CLASSES[variant]} ${className}`}
      {...rest}
    >
      {leadingIcon}
      {children != null && <span>{children}</span>}
      {trailingIcon}
    </button>
  );
});
