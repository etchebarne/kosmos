import React from "react";

interface SettingProps {
  label: string;
  description?: string;
  children: React.ReactNode;
}

export function Setting({ label, description, children }: SettingProps) {
  return (
    <div className="group flex items-center justify-between gap-4 py-2.5 px-3 hover:bg-[var(--color-bg-hover)] border border-transparent hover:border-[var(--color-border-secondary)] transition-colors rounded-md">
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className="text-xs font-medium text-[var(--color-text-primary)]">{label}</span>
        {description && (
          <span className="text-[11px] text-[var(--color-text-secondary)] leading-relaxed">
            {description}
          </span>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
