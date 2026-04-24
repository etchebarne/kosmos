import { useEffect, useState } from "react";
import { Minus, Plus } from "@phosphor-icons/react";

interface NumberInputProps {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
}

export function NumberInput({ value, min, max, step, onChange }: NumberInputProps) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => {
    setDraft(String(value));
  }, [value]);

  const clamp = (n: number) => Math.min(max, Math.max(min, n));
  const commit = () => {
    const n = Number(draft);
    if (Number.isFinite(n)) onChange(clamp(n));
    else setDraft(String(value));
  };

  const atMin = value <= min;
  const atMax = value >= max;

  return (
    <div className="flex items-stretch h-7 text-xs bg-[var(--color-bg-surface)] border border-[var(--color-border-secondary)] hover:border-[var(--color-border-primary)] focus-within:border-[var(--color-accent-blue)] transition-colors rounded-md overflow-hidden">
      <button
        type="button"
        aria-label="Decrease"
        disabled={atMin}
        onClick={() => onChange(clamp(value - step))}
        className="flex items-center justify-center w-6 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-hover)] disabled:opacity-40 disabled:hover:bg-transparent disabled:cursor-not-allowed transition-colors"
      >
        <Minus size={10} weight="bold" />
      </button>
      <input
        type="number"
        value={draft}
        min={min}
        max={max}
        step={step}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            commit();
            (e.currentTarget as HTMLInputElement).blur();
          } else if (e.key === "Escape") {
            setDraft(String(value));
            (e.currentTarget as HTMLInputElement).blur();
          }
        }}
        className="w-12 bg-transparent text-center text-[var(--color-text-primary)] outline-none [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
      />
      <button
        type="button"
        aria-label="Increase"
        disabled={atMax}
        onClick={() => onChange(clamp(value + step))}
        className="flex items-center justify-center w-6 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-hover)] disabled:opacity-40 disabled:hover:bg-transparent disabled:cursor-not-allowed transition-colors"
      >
        <Plus size={10} weight="bold" />
      </button>
    </div>
  );
}
