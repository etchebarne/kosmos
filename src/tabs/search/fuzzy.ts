export function highlightedParts(
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
