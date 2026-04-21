interface SwitchProps {
  value: boolean;
  onChange: (value: boolean) => void;
}

export function Switch({ value, onChange }: SwitchProps) {
  return (
    <button
      className={`flex items-center w-8 h-[18px] p-[1px] border transition-colors rounded-full ${
        value
          ? "bg-[var(--color-accent-blue)] border-[var(--color-accent-blue)]"
          : "bg-[var(--color-bg-surface)] border-[var(--color-border-primary)] hover:border-[var(--color-border-hover)]"
      }`}
      onClick={() => onChange(!value)}
    >
      <span
        className={`w-3.5 h-3.5 bg-white transition-transform rounded-full ${
          value ? "translate-x-3.5" : "translate-x-0"
        }`}
      />
    </button>
  );
}
