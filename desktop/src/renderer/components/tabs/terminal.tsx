import { CanvasAddon } from "@xterm/addon-canvas";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { RefreshCw, SquareTerminal } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Button } from "@/renderer/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/renderer/components/ui/dropdown-menu";
import {
  listTerminalShells,
  openTerminal,
  readTerminalOutput,
  resizeTerminal,
  restartTerminal,
  writeTerminalInput,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import type { TabId, TerminalOutput, TerminalShell, WorkspaceId } from "@/shared/ipc";

type TerminalTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  isActive: boolean;
  onActivatePane(): void;
};

type TerminalDimensions = {
  columns: number;
  rows: number;
};

type TerminalStatus =
  | { kind: "starting" }
  | { kind: "running" }
  | { kind: "exited"; message: string }
  | { kind: "error"; message: string };

const DEFAULT_TERMINAL_SIZE: TerminalDimensions = { columns: 80, rows: 24 };
const MIN_COLUMNS = 10;
const MIN_ROWS = 3;
const POLL_INTERVAL_MS = 50;
const INPUT_FLUSH_DELAY_MS = 8;
const PTY_RESIZE_DEBOUNCE_MS = 200;
const TERMINAL_FONT_FAMILY =
  '"Adwaita Mono", "Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono", monospace';
const TERMINAL_FONT_SIZE = 13;
const TERMINAL_LINE_HEIGHT = 1;

export function TerminalTab({ workspaceId, tabId, isActive, onActivatePane }: TerminalTabProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const applySizeRef = useRef<(() => void) | null>(null);
  const isActiveRef = useRef(isActive);
  const restartRef = useRef<((shell: string) => Promise<boolean>) | null>(null);
  const terminalRef = useRef<XTerm | null>(null);
  const [shells, setShells] = useState<TerminalShell[]>([]);
  const [shellPath, setShellPath] = useState<string | null>(null);
  const [isRestarting, setIsRestarting] = useState(false);
  const [status, setStatus] = useState<TerminalStatus>({ kind: "starting" });

  useEffect(() => {
    let disposed = false;

    void listTerminalShells()
      .then((availableShells) => {
        if (disposed) {
          return;
        }

        setShells(availableShells);
        setShellPath(
          availableShells.find((shell) => shell.isDefault)?.path ?? availableShells[0]?.path ?? null,
        );
      })
      .catch(() => {
        if (!disposed) {
          setShells([]);
        }
      });

    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    isActiveRef.current = isActive;

    if (!isActive) {
      return undefined;
    }

    const frameId = requestAnimationFrame(() => {
      applySizeRef.current?.();
      terminalRef.current?.focus();
    });

    return () => cancelAnimationFrame(frameId);
  }, [isActive]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return undefined;
    }

    let disposed = false;
    let sessionOpen = false;
    let pollInFlight = false;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let inputFlushTimer: ReturnType<typeof setTimeout> | null = null;
    let ptyResizeTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingInput = "";
    let restartInFlight = false;
    let terminalExited = false;
    let size = DEFAULT_TERMINAL_SIZE;
    const terminal = new XTerm({
      cols: size.columns,
      rows: size.rows,
      cursorBlink: true,
      cursorStyle: "block",
      customGlyphs: true,
      fontFamily: TERMINAL_FONT_FAMILY,
      fontSize: TERMINAL_FONT_SIZE,
      letterSpacing: 0,
      lineHeight: TERMINAL_LINE_HEIGHT,
      scrollback: 5000,
      windowOptions: {
        getCellSizePixels: true,
        getWinSizeChars: true,
        getWinSizePixels: true,
      },
      theme: {
        background: "#101010",
        foreground: "#e7e7e7",
        cursor: "#ffffff",
        selectionBackground: "#3f3f46",
        black: "#18181b",
        blue: "#7dd3fc",
        cyan: "#67e8f9",
        green: "#86efac",
        magenta: "#f0abfc",
        red: "#fca5a5",
        white: "#f4f4f5",
        yellow: "#fde68a",
      },
    });
    const canvasAddon = new CanvasAddon();
    const fitAddon = new FitAddon();

    const stopPolling = () => {
      if (pollTimer !== null) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
    };
    const failTerminal = (caughtError: unknown) => {
      const message = errorMessage(caughtError);

      stopPolling();
      terminal.options.disableStdin = true;
      terminal.writeln(`\r\n${message}`);
      setStatus({ kind: "error", message });
    };
    const handleOutput = (output: TerminalOutput): boolean => {
      if (output.truncated) {
        terminal.writeln("\r\n[terminal output truncated]\r\n");
      }

      if (output.output.length > 0) {
        terminal.write(output.output);
      }

      if (output.exited && !terminalExited) {
        const message = terminalExitMessage(output);

        terminalExited = true;
        stopPolling();
        terminal.options.disableStdin = true;
        terminal.writeln(`\r\n${message}`);
        setStatus({ kind: "exited", message });
      }

      return output.exited;
    };
    const pollOutput = async () => {
      if (pollInFlight || disposed) {
        return;
      }

      pollInFlight = true;

      try {
        const output = await readTerminalOutput({ workspaceId, tabId });

        if (!disposed) {
          handleOutput(output);
        }
      } catch (caughtError: unknown) {
        if (!disposed) {
          failTerminal(caughtError);
        }
      } finally {
        pollInFlight = false;
      }
    };
    const startPolling = () => {
      stopPolling();
      pollTimer = setInterval(() => void pollOutput(), POLL_INTERVAL_MS);
    };
    const flushInput = () => {
      inputFlushTimer = null;

      if (pendingInput.length === 0 || !sessionOpen) {
        return;
      }

      const data = pendingInput;
      pendingInput = "";

      void writeTerminalInput({ workspaceId, tabId, data }).catch((caughtError: unknown) => {
        if (!disposed) {
          failTerminal(caughtError);
        }
      });
    };
    const queueInput = (data: string) => {
      pendingInput += data;

      if (inputFlushTimer === null) {
        inputFlushTimer = setTimeout(flushInput, INPUT_FLUSH_DELAY_MS);
      }
    };
    const schedulePtyResize = (nextSize: TerminalDimensions) => {
      if (!sessionOpen) {
        return;
      }

      if (ptyResizeTimer !== null) {
        clearTimeout(ptyResizeTimer);
      }

      ptyResizeTimer = setTimeout(() => {
        ptyResizeTimer = null;

        void resizeTerminal({
          workspaceId,
          tabId,
          columns: nextSize.columns,
          rows: nextSize.rows,
        }).catch((caughtError: unknown) => {
          if (!disposed) {
            failTerminal(caughtError);
          }
        });
      }, PTY_RESIZE_DEBOUNCE_MS);
    };
    const applySize = () => {
      if (!isElementVisible(container)) {
        return;
      }

      const nextSize = fitTerminal(terminal, fitAddon);

      if (dimensionsEqual(size, nextSize)) {
        return;
      }

      size = nextSize;
      terminal.resize(size.columns, size.rows);
      schedulePtyResize(nextSize);
    };
    const startSession = async () => {
      try {
        await nextAnimationFrame();

        if (disposed) {
          return;
        }

        if (isElementVisible(container)) {
          size = fitTerminal(terminal, fitAddon);
        }

        const openedSize = size;
        const output = await openTerminal({
          workspaceId,
          tabId,
          columns: openedSize.columns,
          rows: openedSize.rows,
        });

        if (disposed) {
          return;
        }

        sessionOpen = true;
        setStatus({ kind: "running" });

        if (!dimensionsEqual(openedSize, size)) {
          schedulePtyResize(size);
        }

        if (!handleOutput(output)) {
          startPolling();
        }
      } catch (caughtError: unknown) {
        if (!disposed) {
          failTerminal(caughtError);
        }
      }
    };
    const restartSession = async (shell: string): Promise<boolean> => {
      if (disposed || !sessionOpen || restartInFlight) {
        return false;
      }

      const wasExited = terminalExited;
      restartInFlight = true;
      stopPolling();
      pendingInput = "";
      terminal.options.disableStdin = true;
      setIsRestarting(true);

      if (inputFlushTimer !== null) {
        clearTimeout(inputFlushTimer);
        inputFlushTimer = null;
      }

      if (ptyResizeTimer !== null) {
        clearTimeout(ptyResizeTimer);
        ptyResizeTimer = null;
      }

      try {
        const output = await restartTerminal({
          workspaceId,
          tabId,
          columns: size.columns,
          rows: size.rows,
          shell,
        });

        if (disposed) {
          return false;
        }

        terminal.reset();
        terminal.options.disableStdin = false;
        terminalExited = false;
        setStatus({ kind: "running" });

        if (!handleOutput(output)) {
          startPolling();
        }

        terminal.focus();
        return true;
      } catch (caughtError: unknown) {
        if (!disposed) {
          terminal.writeln(`\r\n${errorMessage(caughtError)}`);
          terminal.options.disableStdin = wasExited;

          if (wasExited) {
            setStatus({ kind: "error", message: errorMessage(caughtError) });
          } else {
            setStatus({ kind: "running" });
            startPolling();
          }
        }

        return false;
      } finally {
        restartInFlight = false;

        if (!disposed) {
          setIsRestarting(false);
        }
      }
    };

    terminal.loadAddon(canvasAddon);
    terminal.loadAddon(fitAddon);
    terminal.open(container);
    terminalRef.current = terminal;
    applySizeRef.current = applySize;
    restartRef.current = restartSession;

    if (isActiveRef.current) {
      terminal.focus();
    }

    const inputDisposable = terminal.onData(queueInput);
    const resizeObserver = new ResizeObserver(applySize);
    resizeObserver.observe(container);
    void startSession();

    return () => {
      disposed = true;
      stopPolling();
      resizeObserver.disconnect();
      inputDisposable.dispose();

      if (inputFlushTimer !== null) {
        clearTimeout(inputFlushTimer);
      }

      if (ptyResizeTimer !== null) {
        clearTimeout(ptyResizeTimer);
      }

      terminal.dispose();
      terminalRef.current = null;
      applySizeRef.current = null;
      restartRef.current = null;
    };
  }, [workspaceId, tabId]);

  const restartSession = async () => {
    if (!shellPath) {
      return;
    }

    await restartRef.current?.(shellPath);
  };

  const changeShell = async (nextShell: string | null) => {
    if (!nextShell || nextShell === shellPath) {
      return;
    }

    const previousShell = shellPath;
    setShellPath(nextShell);

    if (!(await restartRef.current?.(nextShell))) {
      setShellPath(previousShell);
    }
  };

  return (
    <div
      className="terminal-scrollbar-none relative flex h-full min-h-0 flex-col overflow-hidden bg-[#101010] text-white"
      onPointerDown={onActivatePane}
    >
      <div ref={containerRef} className="min-h-0 flex-1 overflow-hidden" />
      {!isRestarting && status.kind !== "running" ? <TerminalStatusBadge status={status} /> : null}
      <div className="flex h-8 shrink-0 items-center justify-end gap-1 border-t border-white/10 bg-[#151515] px-1">
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label="Change terminal shell"
                title="Change terminal shell"
                disabled={shells.length === 0 || status.kind === "starting" || isRestarting}
                className="text-white/65 hover:bg-white/10 hover:text-white"
              />
            }
          >
            <SquareTerminal />
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="end" className="w-32">
            <DropdownMenuRadioGroup
              value={shellPath}
              onValueChange={(value) => void changeShell(value)}
            >
              {shells.map((shell) => (
                <DropdownMenuRadioItem key={shell.path} value={shell.path}>
                  {shell.name}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Restart terminal"
          title="Restart terminal"
          disabled={!shellPath || status.kind === "starting" || isRestarting}
          className="text-white/65 hover:bg-white/10 hover:text-white"
          onClick={() => void restartSession()}
        >
          <RefreshCw className={isRestarting ? "animate-spin" : undefined} />
        </Button>
      </div>
    </div>
  );
}

function TerminalStatusBadge({ status }: { status: TerminalStatus }) {
  const message = terminalStatusMessage(status);

  if (!message) {
    return null;
  }

  return (
    <div className="pointer-events-none absolute top-3 right-3 rounded-full border border-white/10 bg-black/70 px-2.5 py-1 text-[11px] font-medium text-white/75 shadow-lg backdrop-blur">
      {message}
    </div>
  );
}

function terminalStatusMessage(status: TerminalStatus): string | null {
  switch (status.kind) {
    case "starting":
      return "Starting terminal";
    case "running":
      return null;
    case "exited":
    case "error":
      return status.message;
  }
}

function fitTerminal(terminal: XTerm, fitAddon: FitAddon): TerminalDimensions {
  const dimensions = fitAddon.proposeDimensions();

  if (!dimensions) {
    return { columns: terminal.cols, rows: terminal.rows };
  }

  const nextDimensions = {
    columns: Math.max(MIN_COLUMNS, dimensions.cols),
    rows: Math.max(MIN_ROWS, dimensions.rows),
  };

  terminal.resize(nextDimensions.columns, nextDimensions.rows);

  return nextDimensions;
}

function dimensionsEqual(left: TerminalDimensions, right: TerminalDimensions): boolean {
  return left.columns === right.columns && left.rows === right.rows;
}

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

function isElementVisible(element: HTMLElement): boolean {
  return element.getClientRects().length > 0;
}

function terminalExitMessage(output: TerminalOutput): string {
  if (output.signal) {
    return `Terminal exited after signal ${output.signal}`;
  }

  return `Terminal exited with code ${output.exitCode ?? 0}`;
}
