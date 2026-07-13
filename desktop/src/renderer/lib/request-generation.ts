export function isCurrentRequest(requestGeneration: number, currentGeneration: number): boolean {
  return requestGeneration === currentGeneration;
}

export function createRequestGeneration() {
  let current = 0;
  return {
    issue(): number {
      current += 1;
      return current;
    },
    invalidate(): void {
      current += 1;
    },
    isCurrent(generation: number): boolean {
      return isCurrentRequest(generation, current);
    },
  };
}

export function matchesCurrentQuery(
  result: { generation: number; query: string },
  generation: number,
  query: string,
): boolean {
  return result.generation === generation && result.query === query;
}

export function canConsumeRequest(
  pendingGeneration: number | null,
  requestedGeneration: number,
): boolean {
  return pendingGeneration === requestedGeneration;
}
