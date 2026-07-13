import type { editor } from "monaco-editor";

import type { EditorGitLineHunk } from "@/shared/ipc";

import { monaco, resolvedThemeColor } from "./monaco";

type GitDecorationKind = "added" | "deleted" | "modified";

export function editorGitDecorations(
  hunks: EditorGitLineHunk[],
  lineCount: number,
): editor.IModelDeltaDecoration[] {
  if (hunks.length === 0) {
    return [];
  }

  const colors = {
    added: resolvedThemeColor("--diff-added"),
    deleted: resolvedThemeColor("--destructive"),
    modified: resolvedThemeColor("--diff-modified"),
  };

  return hunks.map((hunk) => {
    if (hunk.oldLines === 0) {
      return lineDecoration(hunk.newStart, hunk.newStart + hunk.newLines - 1, "added", lineCount);
    }

    if (hunk.newLines === 0) {
      const line = clampLine(hunk.newStart, lineCount);
      return lineDecoration(line, line, "deleted", lineCount);
    }

    return lineDecoration(hunk.newStart, hunk.newStart + hunk.newLines - 1, "modified", lineCount);
  });

  function lineDecoration(
    startLine: number,
    endLine: number,
    kind: GitDecorationKind,
    totalLines: number,
  ): editor.IModelDeltaDecoration {
    const start = clampLine(startLine, totalLines);
    const end = clampLine(Math.max(startLine, endLine), totalLines);
    const color = colors[kind];
    const deleted = kind === "deleted";

    return {
      range: new monaco.Range(
        start,
        deleted ? Number.MAX_VALUE : 1,
        end,
        deleted ? Number.MAX_VALUE : 1,
      ),
      options: {
        isWholeLine: !deleted,
        linesDecorationsClassName: `kosmos-editor-git-glyph kosmos-editor-git-${kind}`,
        minimap: {
          color,
          position: monaco.editor.MinimapPosition.Gutter,
        },
        overviewRuler: {
          color: `${color}99`,
          position: monaco.editor.OverviewRulerLane.Left,
        },
      },
    };
  }
}

function clampLine(line: number, lineCount: number): number {
  return Math.min(Math.max(line, 1), Math.max(lineCount, 1));
}
