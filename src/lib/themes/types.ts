export interface Theme {
  name: string;
  type: "dark" | "light";

  // ── UI colors ──
  ui: {
    bg: {
      page: string;
      surface: string;
      elevated: string;
      input: string;
      hover: string;
      primary: string;
      tertiary: string;
      projectBar: string;
      tabActive: string;
      tabInactive: string;
    };
    text: {
      primary: string;
      secondary: string;
      tertiary: string;
      muted: string;
    };
    border: {
      primary: string;
      secondary: string;
      divider: string;
    };
    accent: {
      blue: string;
      blueHover: string;
      blueMuted: string;
    };
    status: {
      red: string;
      green: string;
      amber: string;
    };
    scrollbar: {
      track: string;
      hover: string;
      active: string;
    };
  };

  // ── Terminal ANSI colors ──
  terminal: {
    background: string;
    foreground: string;
    cursor: string;
    cursorAccent: string;
    selection: string;
    black: string;
    red: string;
    green: string;
    yellow: string;
    blue: string;
    magenta: string;
    cyan: string;
    white: string;
    brightBlack: string;
    brightRed: string;
    brightGreen: string;
    brightYellow: string;
    brightBlue: string;
    brightMagenta: string;
    brightCyan: string;
    brightWhite: string;
  };

  // ── Editor (Monaco) overrides ──
  editor: {
    background: string;
    foreground: string;
    lineHighlight: string;
    selection: string;
    inactiveSelection: string;
    lineNumber: string;
    lineNumberActive: string;
    cursor: string;
    indentGuide: string;
    indentGuideActive: string;
    widget: string;
    widgetBorder: string;
    suggestBackground: string;
    suggestBorder: string;
    suggestSelected: string;
  };

  // ── Diff viewer ──
  diff: {
    deletionBg: string;
    deletionNumberBg: string;
    deletionHoverBg: string;
    deletionEmphasis: string;
    additionBg: string;
    additionNumberBg: string;
    additionHoverBg: string;
    additionEmphasis: string;
    modificationNumberBg: string;
  };
}
