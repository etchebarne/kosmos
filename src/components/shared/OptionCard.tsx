import type { ReactNode } from "react";

interface OptionCardProps {
  icon: ReactNode;
  label: string;
  onClick: () => void;
}

export function OptionCard({ icon, label, onClick }: OptionCardProps) {
  return (
    <button
      className="flex items-center gap-3 px-3 py-2.5 bg-[var(--color-bg-surface)] border border-[var(--color-border-secondary)] text-left hover:border-[var(--color-accent-blue)] hover:bg-[var(--color-bg-hover)] transition-colors rounded-md"
      onClick={onClick}
    >
      {icon}
      <span className="text-xs text-[var(--color-text-primary)]">{label}</span>
    </button>
  );
}
