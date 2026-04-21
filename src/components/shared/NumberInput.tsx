interface NumberInputProps {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
}

export function NumberInput({ value, min, max, step, onChange }: NumberInputProps) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(e) => onChange(Number(e.target.value))}
      className="text-xs w-16 bg-[var(--color-bg-surface)] border border-[var(--color-border-secondary)] text-[var(--color-text-primary)] px-2 py-1 outline-none hover:border-[var(--color-border-primary)] focus:border-[var(--color-accent-blue)] transition-colors text-center rounded-md"
    />
  );
}
