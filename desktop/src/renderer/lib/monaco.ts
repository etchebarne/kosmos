import * as monaco from "monaco-editor";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import typescriptWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

type MonacoEnvironment = {
  getWorker(moduleId: string, label: string): Worker;
};

const workerScope = self as typeof self & {
  MonacoEnvironment?: MonacoEnvironment;
};

const MONACO_THEME_NAME = "kosmos";

workerScope.MonacoEnvironment = {
  getWorker(_moduleId, label) {
    if (label === "json") {
      return new jsonWorker();
    }

    if (label === "css" || label === "scss" || label === "less") {
      return new cssWorker();
    }

    if (label === "html" || label === "handlebars" || label === "razor") {
      return new htmlWorker();
    }

    if (label === "typescript" || label === "javascript") {
      return new typescriptWorker();
    }

    return new editorWorker();
  },
};

export function applyMonacoTheme(): void {
  const style = getComputedStyle(document.documentElement);
  const context = document.createElement("canvas").getContext("2d");
  if (!context) {
    throw new Error("Could not create a canvas context for the Monaco theme.");
  }
  const color = (name: string) => themeColor(context, style, name);

  monaco.editor.defineTheme(MONACO_THEME_NAME, {
    base: document.documentElement.classList.contains("dark") ? "vs-dark" : "vs",
    inherit: true,
    colors: {
      "diffEditor.border": color("--border"),
      "diffEditor.insertedLineBackground": withAlpha(color("--diff-added"), "18"),
      "diffEditor.insertedTextBackground": withAlpha(color("--diff-added"), "38"),
      "diffEditor.removedLineBackground": withAlpha(color("--destructive"), "18"),
      "diffEditor.removedTextBackground": withAlpha(color("--destructive"), "38"),
      "diffEditor.unchangedCodeBackground": withAlpha(color("--muted"), "40"),
      "diffEditor.unchangedRegionBackground": color("--background"),
      "diffEditor.unchangedRegionForeground": color("--muted-foreground"),
      "diffEditorGutter.insertedLineBackground": withAlpha(color("--diff-added"), "24"),
      "diffEditorGutter.removedLineBackground": withAlpha(color("--destructive"), "24"),
      "editor.background": color("--background"),
      "editor.foreground": color("--foreground"),
      "editorGutter.background": color("--background"),
      "editor.lineHighlightBackground": color("--muted"),
      "editor.selectionBackground": color("--accent"),
      "editor.inactiveSelectionBackground": color("--secondary"),
      "editor.selectionHighlightBackground": color("--muted"),
      "editorCursor.foreground": color("--foreground"),
      "editorLineNumber.foreground": color("--muted-foreground"),
      "editorLineNumber.activeForeground": color("--foreground"),
      "editorIndentGuide.background1": color("--border"),
      "editorIndentGuide.activeBackground1": color("--ring"),
      "editorWidget.background": color("--popover"),
      "editorWidget.border": color("--border"),
      "editorHoverWidget.background": color("--popover"),
      "editorHoverWidget.border": color("--border"),
      "editorSuggestWidget.background": color("--popover"),
      "editorSuggestWidget.border": color("--border"),
      "input.background": color("--popover"),
      "input.foreground": color("--foreground"),
      "input.border": color("--input"),
      "editor.findMatchBackground": color("--accent"),
      "editor.findMatchHighlightBackground": color("--secondary"),
      "editorError.foreground": color("--destructive"),
      "editorBracketHighlight.foreground1": color("--editor-syntax-bracket-1"),
      "editorBracketHighlight.foreground2": color("--editor-syntax-bracket-2"),
      "editorBracketHighlight.foreground3": color("--editor-syntax-bracket-3"),
    },
    rules: [
      { token: "comment", foreground: tokenColor(color("--editor-syntax-comment")), fontStyle: "italic" },
      { token: "keyword", foreground: tokenColor(color("--editor-syntax-keyword")) },
      { token: "string", foreground: tokenColor(color("--editor-syntax-string")) },
      { token: "number", foreground: tokenColor(color("--editor-syntax-number")) },
      { token: "type", foreground: tokenColor(color("--editor-syntax-type")) },
      { token: "function", foreground: tokenColor(color("--editor-syntax-function")) },
      { token: "variable", foreground: tokenColor(color("--editor-syntax-variable")) },
      { token: "tag", foreground: tokenColor(color("--editor-syntax-tag")) },
      { token: "attribute", foreground: tokenColor(color("--editor-syntax-attribute")) },
      { token: "operator", foreground: tokenColor(color("--editor-syntax-operator")) },
      { token: "regexp", foreground: tokenColor(color("--editor-syntax-regexp")) },
    ],
  });
  monaco.editor.setTheme(MONACO_THEME_NAME);
}

function themeColor(
  context: CanvasRenderingContext2D,
  style: CSSStyleDeclaration,
  name: string,
): string {
  const color = style.getPropertyValue(name).trim();

  if (!color) {
    throw new Error(`Missing theme color ${name}.`);
  }

  context.clearRect(0, 0, 1, 1);
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);

  const components = context.getImageData(0, 0, 1, 1).data;
  const red = components[0] ?? 0;
  const green = components[1] ?? 0;
  const blue = components[2] ?? 0;
  const alpha = components[3] ?? 0;
  const hex = [red, green, blue, alpha]
    .map((component) => component.toString(16).padStart(2, "0"))
    .join("");

  return alpha === 255 ? `#${hex.slice(0, 6)}` : `#${hex}`;
}

function tokenColor(color: string): string {
  return color.slice(1);
}

function withAlpha(color: string, alpha: string): string {
  return `${color.slice(0, 7)}${alpha}`;
}

export { monaco };
