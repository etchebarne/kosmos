import type { editor } from "monaco-editor";

type ChangeType = "added" | "modified" | "deleted";

interface LineChange {
  type: ChangeType;
  /** First line in the new file (1-based). For "deleted", points to the line *after* the deletion. */
  startLine: number;
  /** Last line in the new file (1-based). For "deleted", equals startLine. */
  endLine: number;
}

/** Parse a unified diff into new-side line ranges labelled added/modified/deleted. */
export function parseDiffChanges(patch: string): LineChange[] {
  const changes: LineChange[] = [];
  const lines = patch.split("\n");

  let newLine = 0;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    const hunkMatch = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunkMatch) {
      newLine = parseInt(hunkMatch[1], 10);
      continue;
    }

    if (newLine === 0) continue;

    if (line.startsWith(" ")) {
      newLine++;
      continue;
    }

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
        // Gutter marker sits on the line above the deletion.
        const marker = addStart > 1 ? addStart - 1 : 1;
        changes.push({ type: "deleted", startLine: marker, endLine: marker });
      }

      i = j - 1;
      continue;
    }
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
      marginClassName: DECORATION_CLASS[c.type],
    },
  }));
}
