import type { editor } from "monaco-editor";

export type ChangeType = "added" | "modified" | "deleted";

export interface LineChange {
  type: ChangeType;
  /** First line in the new file (1-based). For "deleted", points to the line *after* the deletion. */
  startLine: number;
  /** Last line in the new file (1-based). For "deleted", equals startLine. */
  endLine: number;
}

/**
 * Parse a unified diff string and extract line-level changes for the new side.
 *
 * Groups consecutive -/+ runs within each hunk:
 * - Only + lines → "added"
 * - Only - lines → "deleted" (placed at the new-side position)
 * - Both - and + → "modified" (the + line range)
 */
export function parseDiffChanges(patch: string): LineChange[] {
  const changes: LineChange[] = [];
  const lines = patch.split("\n");

  let newLine = 0; // current new-side line number

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Hunk header: @@ -oldStart[,oldCount] +newStart[,newCount] @@
    const hunkMatch = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunkMatch) {
      newLine = parseInt(hunkMatch[1], 10);
      continue;
    }

    // Skip file headers and other non-content lines
    if (newLine === 0) continue;

    if (line.startsWith(" ")) {
      newLine++;
      continue;
    }

    // Accumulate a change block: consecutive - and + lines
    if (line.startsWith("-") || line.startsWith("+")) {
      let deletions = 0;
      let additions = 0;
      const addStart = newLine;
      let j = i;

      while (j < lines.length) {
        const cl = lines[j];
        if (cl.startsWith("-")) {
          deletions++;
          j++;
        } else if (cl.startsWith("+")) {
          additions++;
          newLine++;
          j++;
        } else {
          break;
        }
      }

      if (additions > 0 && deletions > 0) {
        changes.push({ type: "modified", startLine: addStart, endLine: addStart + additions - 1 });
      } else if (additions > 0) {
        changes.push({ type: "added", startLine: addStart, endLine: addStart + additions - 1 });
      } else if (deletions > 0) {
        // Mark the line before the deletion (or line 1 if at the start)
        const marker = addStart > 1 ? addStart - 1 : 1;
        changes.push({ type: "deleted", startLine: marker, endLine: marker });
      }

      i = j - 1; // outer loop will i++
      continue;
    }

    // No-newline-at-end-of-file marker or other non-content line
    // Don't advance newLine
  }

  return changes;
}

const DECORATION_CLASS: Record<ChangeType, string> = {
  added: "diff-gutter-added",
  modified: "diff-gutter-modified",
  deleted: "diff-gutter-deleted",
};

/** Build Monaco decorations from parsed diff changes. */
export function buildDiffDecorations(changes: LineChange[]): editor.IModelDeltaDecoration[] {
  return changes.map((c) => ({
    range: {
      startLineNumber: c.startLine,
      startColumn: 1,
      endLineNumber: c.endLine,
      endColumn: 1,
    },
    options: {
      isWholeLine: true,
      linesDecorationsClassName: DECORATION_CLASS[c.type],
    },
  }));
}
