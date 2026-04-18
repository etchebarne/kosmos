import { useEffect, useRef, useState, useCallback, forwardRef, useImperativeHandle } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { ArrowsClockwise, CaretDown, Eraser, Minus, Plus, Folder } from "@phosphor-icons/react";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { useClickOutside } from "../../hooks/useClickOutside";
import { StateView } from "../../components/shared/StateView";
import { getTheme } from "../../lib/themes";
import { DEFAULT_FONT_SIZE, MIN_FONT_SIZE, MAX_FONT_SIZE } from "../../store/editor.store";
import type { TabContentProps } from "../types";
import "@xterm/xterm/css/xterm.css";

interface ShellInfo {
  name: string;
  program: string;
  args: string[];
}

interface TerminalViewHandle {
  clear: () => void;
}

let spawnCounter = 0;

const TerminalView = forwardRef<
  TerminalViewHandle,
  {
    tabId: string;
    shell: ShellInfo;
    cwd: string;
    fontSize: number;
    onFontSizeChange: (size: number) => void;
  }
>(function TerminalView({ tabId, shell, cwd, fontSize, onFontSizeChange }, ref) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const fontSizeRef = useRef(fontSize);
  fontSizeRef.current = fontSize;
  const onFontSizeChangeRef = useRef(onFontSizeChange);
  onFontSizeChangeRef.current = onFontSizeChange;

  useImperativeHandle(
    ref,
    () => ({
      clear: () => {
        terminalRef.current?.clear();
      },
    }),
    [],
  );

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    // Unique per effect run so Strict Mode's double-invoke doesn't race the PTY.
    const terminalId = `${tabId}-${++spawnCounter}`;

    const t = getTheme().terminal;
    const terminal = new Terminal({
      fontSize: fontSizeRef.current,
      fontFamily: "'Cascadia Code', 'Consolas', 'Courier New', monospace",
      cursorBlink: true,
      theme: {
        background: t.background,
        foreground: t.foreground,
        cursor: t.cursor,
        cursorAccent: t.cursorAccent,
        selectionBackground: t.selection,
        black: t.black,
        red: t.red,
        green: t.green,
        yellow: t.yellow,
        blue: t.blue,
        magenta: t.magenta,
        cyan: t.cyan,
        white: t.white,
        brightBlack: t.brightBlack,
        brightRed: t.brightRed,
        brightGreen: t.brightGreen,
        brightYellow: t.brightYellow,
        brightBlue: t.brightBlue,
        brightMagenta: t.brightMagenta,
        brightCyan: t.brightCyan,
        brightWhite: t.brightWhite,
      },
    });
    terminalRef.current = terminal;

    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    fitAddonRef.current = fitAddon;

    // Intercept shortcuts before they reach the PTY.
    terminal.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey) return true;
      // Ctrl+C copies if there's a selection, otherwise passes SIGINT through.
      if (!e.shiftKey && e.key === "c") {
        if (terminal.hasSelection()) {
          e.preventDefault();
          navigator.clipboard.writeText(terminal.getSelection());
          terminal.clearSelection();
          return false;
        }
        return true;
      }
      if (e.key === "V" || e.key === "v") {
        e.preventDefault();
        (async () => {
          try {
            const text = await readText();
            if (text) {
              terminal.paste(text);
              return;
            }
          } catch {}
          // Clipboard held an image; forward it so TUI apps (wsl shims / xclip) can paste it.
          await invoke("terminal_forward_clipboard_image", { id: terminalId }).catch((err) => {
            console.warn("clipboard image forward:", err);
          });
          invoke("terminal_write", { id: terminalId, data: "\x16" });
        })();
        return false;
      }
      if (e.key === "=" || e.key === "+") {
        e.preventDefault();
        const cur = terminal.options.fontSize ?? DEFAULT_FONT_SIZE;
        onFontSizeChangeRef.current(Math.min(cur + 1, MAX_FONT_SIZE));
        return false;
      }
      if (e.key === "-") {
        e.preventDefault();
        const cur = terminal.options.fontSize ?? DEFAULT_FONT_SIZE;
        onFontSizeChangeRef.current(Math.max(cur - 1, MIN_FONT_SIZE));
        return false;
      }
      if (e.key === "0") {
        e.preventDefault();
        onFontSizeChangeRef.current(DEFAULT_FONT_SIZE);
        return false;
      }
      return true;
    });

    terminal.open(el);

    let disposed = false;
    let spawned = false;
    let reconnecting = false;
    const cleanups: (() => void)[] = [];

    const spawnTerminal = () =>
      invoke("terminal_spawn", {
        id: terminalId,
        program: shell.program,
        args: shell.args,
        cwd,
        cols: terminal.cols,
        rows: terminal.rows,
      });

    const attemptReconnect = async () => {
      if (reconnecting || disposed) return;
      reconnecting = true;
      terminal.write("\r\n\x1b[33m[Reconnecting...]\x1b[0m\r\n");
      try {
        await spawnTerminal();
        if (disposed) {
          invoke("terminal_close", { id: terminalId });
          return;
        }
        // Forcing a resize nudges TUIs to redraw after reconnect.
        fitAddon.fit();
        invoke("terminal_resize", {
          id: terminalId,
          cols: terminal.cols,
          rows: terminal.rows,
        });
        terminal.write("\x1b[32m[Connected]\x1b[0m\r\n");
      } catch {
        if (!disposed) {
          terminal.write("\x1b[31m[Failed to reconnect]\x1b[0m\r\n");
        }
      } finally {
        reconnecting = false;
      }
    };

    let resizeTimeout: ReturnType<typeof setTimeout>;
    const observer = new ResizeObserver(() => {
      clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(() => {
        // display:none collapses the container to 0×0; fitting then corrupts TUIs.
        if (!el.offsetWidth && !el.offsetHeight) return;

        fitAddon.fit();
        if (spawned) {
          invoke("terminal_resize", {
            id: terminalId,
            cols: terminal.cols,
            rows: terminal.rows,
          });
        }
      }, 150);
    });
    observer.observe(el);
    cleanups.push(() => {
      clearTimeout(resizeTimeout);
      observer.disconnect();
    });

    // Moving the DOM between panes can clear xterm's canvas; re-fit and repaint.
    const onPaneChanged = () => {
      requestAnimationFrame(() => {
        if (disposed) return;
        fitAddon.fit();
        terminal.refresh(0, terminal.rows - 1);
        if (spawned) {
          invoke("terminal_resize", {
            id: terminalId,
            cols: terminal.cols,
            rows: terminal.rows,
          });
        }
      });
    };
    el.addEventListener("pane-changed", onPaneChanged);
    cleanups.push(() => el.removeEventListener("pane-changed", onPaneChanged));

    const onThemeChanged = () => {
      const nt = getTheme().terminal;
      terminal.options.theme = {
        background: nt.background,
        foreground: nt.foreground,
        cursor: nt.cursor,
        cursorAccent: nt.cursorAccent,
        selectionBackground: nt.selection,
        black: nt.black,
        red: nt.red,
        green: nt.green,
        yellow: nt.yellow,
        blue: nt.blue,
        magenta: nt.magenta,
        cyan: nt.cyan,
        white: nt.white,
        brightBlack: nt.brightBlack,
        brightRed: nt.brightRed,
        brightGreen: nt.brightGreen,
        brightYellow: nt.brightYellow,
        brightBlue: nt.brightBlue,
        brightMagenta: nt.brightMagenta,
        brightCyan: nt.brightCyan,
        brightWhite: nt.brightWhite,
      };
    };
    window.addEventListener("theme-changed", onThemeChanged);
    cleanups.push(() => window.removeEventListener("theme-changed", onThemeChanged));

    // Two rAFs let layout complete before fit() so the container has final dims.
    requestAnimationFrame(() => {
      requestAnimationFrame(async () => {
        if (disposed) return;

        fitAddon.fit();

        // Register listeners BEFORE spawn — prompt can arrive before spawn() returns.
        const unlisten = await listen<string>(`terminal-data-${terminalId}`, (event) => {
          terminal.write(event.payload);
        });
        cleanups.push(unlisten);

        const unlistenExit = await listen<number | null>(`terminal-exit-${terminalId}`, (event) => {
          const code = event.payload;
          const msg = code != null ? `Process exited (code ${code})` : "Process exited";
          terminal.write(`\r\n\x1b[90m[${msg}]\x1b[0m\r\n`);
          terminal.write("\x1b[90m[Press Enter to restart]\x1b[0m");
          const restartHandler = terminal.onData((data) => {
            if (data === "\r" || data === "\n") {
              restartHandler.dispose();
              terminal.write("\r\n");
              spawnTerminal()
                .then(() => {
                  terminal.write("\x1b[32m[Restarted]\x1b[0m\r\n");
                  fitAddon.fit();
                  invoke("terminal_resize", {
                    id: terminalId,
                    cols: terminal.cols,
                    rows: terminal.rows,
                  });
                })
                .catch((err) => {
                  terminal.write(`\x1b[31m[Failed to restart: ${err}]\x1b[0m\r\n`);
                });
            }
          });
          cleanups.push(() => restartHandler.dispose());
        });
        cleanups.push(unlistenExit);

        try {
          await spawnTerminal();
        } catch (e) {
          terminal.write(`\x1b[31mFailed to start shell: ${e}\x1b[0m\r\n`);
          return;
        }

        if (disposed) {
          invoke("terminal_close", { id: terminalId });
          return;
        }

        spawned = true;

        // Keystrokes → PTY; a write failure means the agent died, so reconnect.
        const onData = terminal.onData((data) => {
          if (reconnecting) return;
          invoke("terminal_write", { id: terminalId, data }).catch(() => {
            attemptReconnect();
          });
        });
        cleanups.push(() => onData.dispose());
      });
    });

    return () => {
      disposed = true;
      cleanups.forEach((fn) => fn());
      terminal.dispose();
      invoke("terminal_close", { id: terminalId });
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [tabId, shell, cwd]);

  useEffect(() => {
    const term = terminalRef.current;
    const fit = fitAddonRef.current;
    if (!term || !fit) return;
    if (term.options.fontSize === fontSize) return;
    term.options.fontSize = fontSize;
    fit.fit();
  }, [fontSize]);

  return <div ref={containerRef} className="w-full h-full overflow-hidden" />;
});

function BarButton({
  onClick,
  title,
  disabled,
  children,
}: {
  onClick: () => void;
  title: string;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={title}
      title={title}
      onClick={onClick}
      disabled={disabled}
      className="h-full px-1.5 flex items-center text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
    >
      {children}
    </button>
  );
}

function InlineShellPicker({
  shells,
  activeIndex,
  onSelect,
}: {
  shells: ShellInfo[];
  activeIndex: number;
  onSelect: (index: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useClickOutside(ref, () => setOpen(false), open);
  const active = shells[activeIndex];

  return (
    <div ref={ref} className="relative h-full flex items-center">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="h-full px-1.5 flex items-center gap-1 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer"
      >
        <span className="font-ui">{active?.name ?? "—"}</span>
        <CaretDown
          size={9}
          className={`shrink-0 transition-transform duration-200 ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open && shells.length > 0 && (
        <div className="absolute right-0 bottom-full mb-1 min-w-full py-1 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] shadow-[3px_3px_0_rgba(0,0,0,0.25)] rounded-md z-50 animate-fade-in-up origin-bottom">
          {shells.map((s, i) => (
            <button
              key={`${s.program}-${i}`}
              type="button"
              onClick={() => {
                onSelect(i);
                setOpen(false);
              }}
              className={`block w-full text-left px-2.5 py-1 text-[11px] whitespace-nowrap transition-colors font-ui ${
                i === activeIndex
                  ? "text-[var(--color-text-primary)] bg-[var(--color-bg-input)]"
                  : "text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-input)] hover:text-[var(--color-text-primary)]"
              }`}
            >
              {s.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function TerminalStatusBar({
  shells,
  activeIndex,
  onSelectShell,
  fontSize,
  onFontSizeChange,
  onClear,
  onRestart,
  cwdLabel,
}: {
  shells: ShellInfo[];
  activeIndex: number;
  onSelectShell: (index: number) => void;
  fontSize: number;
  onFontSizeChange: (size: number) => void;
  onClear: () => void;
  onRestart: () => void;
  cwdLabel: string;
}) {
  return (
    <div className="h-6 flex items-center justify-between gap-2 pl-2 border-t border-[var(--color-border-primary)] bg-[var(--color-bg-surface)] text-[11px] shrink-0">
      <div className="flex items-center gap-1.5 min-w-0">
        <Folder size={11} className="text-[var(--color-text-tertiary)] shrink-0" />
        <span className="text-[var(--color-text-secondary)] truncate font-ui">{cwdLabel}</span>
      </div>

      <div className="flex items-stretch shrink-0 h-full">
        <BarButton
          onClick={() => onFontSizeChange(Math.max(fontSize - 1, MIN_FONT_SIZE))}
          title="Zoom out"
          disabled={fontSize <= MIN_FONT_SIZE}
        >
          <Minus size={11} />
        </BarButton>
        <button
          type="button"
          onClick={() => onFontSizeChange(DEFAULT_FONT_SIZE)}
          title="Reset zoom"
          className="h-full px-1 flex items-center text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer font-ui tabular-nums"
        >
          {fontSize}
        </button>
        <BarButton
          onClick={() => onFontSizeChange(Math.min(fontSize + 1, MAX_FONT_SIZE))}
          title="Zoom in"
          disabled={fontSize >= MAX_FONT_SIZE}
        >
          <Plus size={11} />
        </BarButton>

        <div className="self-center w-px h-3 bg-[var(--color-border-primary)] mx-1" />

        {shells.length > 0 && (
          <InlineShellPicker shells={shells} activeIndex={activeIndex} onSelect={onSelectShell} />
        )}

        <div className="self-center w-px h-3 bg-[var(--color-border-primary)] mx-1" />

        <BarButton onClick={onClear} title="Clear">
          <Eraser size={12} />
        </BarButton>
        <BarButton onClick={onRestart} title="Restart">
          <ArrowsClockwise size={12} />
        </BarButton>
      </div>
    </div>
  );
}

export function TerminalTab({ tab }: TabContentProps) {
  const workspace = useActiveWorkspace();
  const [shells, setShells] = useState<ShellInfo[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [loading, setLoading] = useState(true);
  const [fontSize, setFontSize] = useState(DEFAULT_FONT_SIZE);
  const [restartEpoch, setRestartEpoch] = useState(0);
  const viewRef = useRef<TerminalViewHandle>(null);

  useEffect(() => {
    invoke<ShellInfo[]>("terminal_list_shells", {
      workspacePath: workspace?.path ?? null,
    }).then((s) => {
      setShells(s);
      setActiveIndex(0);
      setLoading(false);
    });
  }, [workspace?.path]);

  const handleSelectShell = useCallback((index: number) => {
    setActiveIndex(index);
  }, []);

  const handleClear = useCallback(() => {
    viewRef.current?.clear();
  }, []);

  const handleRestart = useCallback(() => {
    setRestartEpoch((e) => e + 1);
  }, []);

  if (!workspace) {
    return <StateView message="No workspace open" />;
  }

  const activeShell = shells[activeIndex];

  return (
    <div className="flex flex-col h-full bg-[var(--color-bg-page)]">
      <div className="flex-1 min-h-0 relative">
        {loading ? (
          <div className="flex items-center justify-center h-full">
            <p className="text-xs text-[var(--color-text-secondary)]">Detecting shells...</p>
          </div>
        ) : !activeShell ? (
          <div className="flex items-center justify-center h-full">
            <p className="text-xs text-[var(--color-text-muted)]">No shells found</p>
          </div>
        ) : (
          <TerminalView
            key={`${restartEpoch}-${activeShell.program}-${activeShell.args.join(" ")}`}
            ref={viewRef}
            tabId={tab.id}
            shell={activeShell}
            cwd={workspace.path}
            fontSize={fontSize}
            onFontSizeChange={setFontSize}
          />
        )}
      </div>
      <TerminalStatusBar
        shells={shells}
        activeIndex={activeIndex}
        onSelectShell={handleSelectShell}
        fontSize={fontSize}
        onFontSizeChange={setFontSize}
        onClear={handleClear}
        onRestart={handleRestart}
        cwdLabel={workspace.name}
      />
    </div>
  );
}
