import { useRef, useEffect } from "react";

const INDENT_SIZE = 16;
const LEFT_PAD = 8;

export function InlineInput({
  depth,
  iconNode,
  defaultValue,
  onConfirm,
  onCancel,
}: {
  depth: number;
  iconNode: React.ReactNode;
  defaultValue: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    const dotIndex = defaultValue.lastIndexOf(".");
    if (dotIndex > 0) {
      el.setSelectionRange(0, dotIndex);
    } else {
      el.select();
    }
  }, [defaultValue]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      const value = inputRef.current?.value.trim();
      if (value) onConfirm(value);
      else onCancel();
    } else if (e.key === "Escape") {
      onCancel();
    }
  };

  return (
    <div
      className="relative flex items-center w-full h-[28px] gap-1.5"
      style={{ paddingLeft: LEFT_PAD + depth * INDENT_SIZE }}
    >
      {Array.from({ length: depth }, (_, i) => (
        <span
          key={i}
          className="absolute top-0 bottom-0 w-px bg-[var(--color-border-primary)] opacity-40"
          style={{ left: LEFT_PAD + i * INDENT_SIZE + 8 }}
        />
      ))}
      <span className="w-4 h-4 shrink-0" />
      {iconNode}
      <input
        ref={inputRef}
        className="flex-1 text-[13px] bg-[var(--color-bg-input)] text-[var(--color-text-primary)] border border-[var(--color-border-focus)] outline-none px-1 min-w-0"
        defaultValue={defaultValue}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          const value = inputRef.current?.value.trim();
          if (value && value !== defaultValue) onConfirm(value);
          else onCancel();
        }}
      />
    </div>
  );
}
