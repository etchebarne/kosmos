function highlightedParts(
  text: string,
  indices: number[],
): { text: string; highlighted: boolean }[] {
  const set = new Set(indices);
  const parts: { text: string; highlighted: boolean }[] = [];
  let current = "";
  let isHighlighted = set.has(0);

  for (let i = 0; i < text.length; i++) {
    const h = set.has(i);
    if (h !== isHighlighted) {
      if (current) parts.push({ text: current, highlighted: isHighlighted });
      current = "";
      isHighlighted = h;
    }
    current += text[i];
  }
  if (current) parts.push({ text: current, highlighted: isHighlighted });

  return parts;
}

/** Render `text` with `indices` highlighted in accent blue. */
export function HighlightedText({ text, indices }: { text: string; indices: number[] }) {
  return (
    <>
      {highlightedParts(text, indices).map((p, i) =>
        p.highlighted ? (
          <span key={i} className="text-[var(--color-accent-blue)] font-semibold">
            {p.text}
          </span>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}
