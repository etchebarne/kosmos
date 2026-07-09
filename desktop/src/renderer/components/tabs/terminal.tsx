import { CanvasAddon } from "@xterm/addon-canvas";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";

import {
  openTerminal,
  readTerminalOutput,
  resizeTerminal,
  writeTerminalInput,
} from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import type { TabId, TerminalOutput, WorkspaceId } from "@/shared/ipc";

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
  const terminalRef = useRef<XTerm | null>(null);
  const [status, setStatus] = useState<TerminalStatus>({ kind: "starting" });

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

    terminal.loadAddon(canvasAddon);
    terminal.loadAddon(fitAddon);
    terminal.open(container);
    terminalRef.current = terminal;
    applySizeRef.current = applySize;

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
    };
  }, [workspaceId, tabId]);

  return (
    <div
      className="terminal-scrollbar-none relative h-full min-h-0 overflow-hidden bg-[#101010] text-white"
      onPointerDown={onActivatePane}
    >
      <div ref={containerRef} className="h-full min-h-0 overflow-hidden" />
      {status.kind !== "running" ? <TerminalStatusBadge status={status} /> : null}
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
