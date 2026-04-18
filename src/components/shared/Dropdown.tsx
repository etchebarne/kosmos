import { useState, useRef } from "react";
import { CaretDown } from "@phosphor-icons/react";
import { useClickOutside } from "../../hooks/useClickOutside";

export interface DropdownOption {
  value: string;
  label: string;
}

interface DropdownProps {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
}

export function Dropdown({ value, options, onChange }: DropdownProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selected = options.find((o) => o.value === value);

  useClickOutside(ref, () => setOpen(false), open);

  return (
    <div ref={ref} className="relative">
      <button
        className={`flex items-center justify-between gap-3 text-xs px-3 py-1.5 min-w-[120px] border transition-colors bg-[var(--color-bg-surface)] text-[var(--color-text-primary)] rounded-md ${
          open
            ? "border-[var(--color-accent-blue)]"
            : "border-[var(--color-border-secondary)] hover:border-[var(--color-border-primary)]"
        }`}
        onClick={() => setOpen(!open)}
      >
        <span className="whitespace-nowrap overflow-hidden text-ellipsis">
          {selected?.label ?? value}
        </span>
        <CaretDown
          size={12}
          className={`shrink-0 text-[var(--color-text-tertiary)] transition-transform duration-200 ${open ? "rotate-180" : ""}`}
        />
      </button>

      {open && (
        <div className="absolute right-0 top-full mt-1 z-50 min-w-full py-1 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] shadow-[3px_3px_0_rgba(0,0,0,0.25)] animate-fade-in-down origin-top rounded-md">
          {options.map((opt) => (
            <button
              key={opt.value}
              className={`w-full text-left px-2.5 py-1.5 text-xs transition-colors ${
                opt.value === value
                  ? "text-[var(--color-text-primary)] bg-[var(--color-bg-input)]"
                  : "text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-input)] hover:text-[var(--color-text-primary)]"
              }`}
              onClick={() => {
                onChange(opt.value);
                setOpen(false);
              }}
            >
              {opt.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
