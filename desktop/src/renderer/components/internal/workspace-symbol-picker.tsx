import { useEffect, useRef, useState } from "react";

import {
  getLanguageServerWorkspaceSymbols,
  resolveLanguageServerWorkspaceSymbol,
  type RequestCancellation,
} from "@/renderer/ipc";
import { appShortcutForEvent } from "@/renderer/lib/app-shortcuts";
import { resolvedWorkspaceSymbolIsCurrent } from "@/renderer/lib/language-feature-adapters";
import { hasRunningLanguageServer } from "@/renderer/lib/language-server-state";
import { matchesCurrentQuery } from "@/renderer/lib/request-generation";
import { useLanguageServerStore } from "@/renderer/stores/language-server-store";
import {
  LANGUAGE_SERVERS_SETTINGS_SECTION,
  useSettingsDialogStore,
} from "@/renderer/stores/settings-dialog-store";
import { useWorkspaceStore } from "@/renderer/stores/workspace-store";
import type { LanguageServerWorkspaceSymbol } from "@/shared/ipc";

import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";

export function WorkspaceSymbolPicker() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<{
    generation: number;
    query: string;
    symbols: LanguageServerWorkspaceSymbol[];
  }>({ generation: 0, query: "", symbols: [] });
  const [loading, setLoading] = useState(false);
  const cancellationRef = useRef<ReturnType<typeof cancellationSource> | null>(null);
  const openCancellationRef = useRef<ReturnType<typeof cancellationSource> | null>(null);
  const queryGenerationRef = useRef(0);
  const languageServersReady = useLanguageServersReady(open);
  const languageServerRunning = useLanguageServerStore((state) => hasRunningLanguageServer(state.servers));
  const languageServerStatusFailed = useLanguageServerStore((state) => state.error !== null);
  const showLanguageServerHint =
    languageServersReady && !languageServerStatusFailed && !languageServerRunning;

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (appShortcutForEvent(event) === "workspaceSymbols") {
        event.preventDefault();
        event.stopPropagation();
        setOpen(true);
      }
    };
    window.addEventListener("keydown", listener, true);
    return () => window.removeEventListener("keydown", listener, true);
  }, []);

  useEffect(() => {
    if (!open) {
      cancellationRef.current?.cancel();
      openCancellationRef.current?.cancel();
      setQuery("");
      setResults({ generation: queryGenerationRef.current, query: "", symbols: [] });
      setLoading(false);
      return undefined;
    }
    const generation = queryGenerationRef.current;
    cancellationRef.current?.cancel();
    cancellationRef.current = null;
    const timeout = window.setTimeout(() => {
      const source = cancellationSource();
      cancellationRef.current = source;
      setLoading(true);
      void getLanguageServerWorkspaceSymbols({ query }, source.token)
        .then((nextSymbols) => {
          if (
            !source.token.isCancellationRequested &&
            matchesCurrentQuery({ generation, query }, queryGenerationRef.current, query)
          ) {
            setResults({ generation, query, symbols: nextSymbols.slice(0, 200) });
          }
        })
        .catch(() => {
          if (!source.token.isCancellationRequested) {
            setResults({ generation, query, symbols: [] });
          }
        })
        .finally(() => {
          if (cancellationRef.current === source) {
            setLoading(false);
          }
        });
    }, query ? 100 : 0);
    return () => {
      window.clearTimeout(timeout);
      if (cancellationRef.current) {
        cancellationRef.current.cancel();
        cancellationRef.current = null;
      }
    };
  }, [open, query]);

  const openSymbol = async (symbol: LanguageServerWorkspaceSymbol, generation: number) => {
    if (!matchesCurrentQuery({ generation, query: results.query }, queryGenerationRef.current, query)) {
      return;
    }
    openCancellationRef.current?.cancel();
    const source = cancellationSource();
    openCancellationRef.current = source;
    let selected = symbol;
    if (!selected.location && selected.resolveSupported) {
      try {
        const resolved = await resolveLanguageServerWorkspaceSymbol({
          serverId: selected.serverId,
          workspaceId: selected.workspaceId,
          raw: selected.raw,
        }, source.token);
        if (
          source.token.isCancellationRequested ||
          !matchesCurrentQuery({ generation, query: results.query }, queryGenerationRef.current, query) ||
          !resolvedWorkspaceSymbolIsCurrent(selected, resolved)
        ) {
          return;
        }
        selected = resolved;
      } catch {
        return;
      }
    }
    if (!selected.location) {
      return;
    }
    if (!matchesCurrentQuery({ generation, query: results.query }, queryGenerationRef.current, query)) {
      return;
    }
    const location = selected.location;
    setOpen(false);
    await useWorkspaceStore.getState().openEditorLocation(
      location.workspaceId,
      location.path,
      location.selectionRange.start.line + 1,
      location.selectionRange.start.character + 1,
      location.selectionRange.end.line + 1,
      location.selectionRange.end.character + 1,
    );
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        className="top-[18%] !flex max-h-[min(30rem,70vh)] max-w-xl translate-y-0 flex-col gap-0 overflow-hidden p-0"
        showCloseButton={false}
      >
        <DialogTitle className="sr-only">Workspace symbols</DialogTitle>
        <Input
          autoFocus
          className="h-11 rounded-none border-x-0 border-t-0 px-4 shadow-none focus-visible:ring-0"
          placeholder="Go to symbol in workspace"
          value={query}
          onChange={(event) => {
            queryGenerationRef.current += 1;
            setQuery(event.target.value);
            setResults({
              generation: queryGenerationRef.current,
              query: event.target.value,
              symbols: [],
            });
          }}
          onKeyDown={(event) => {
            const symbol = results.symbols[0];
            if (
              event.key === "Enter" &&
              symbol &&
              matchesCurrentQuery(results, queryGenerationRef.current, query)
            ) {
              event.preventDefault();
              void openSymbol(symbol, results.generation);
            }
          }}
        />
        <div className="min-h-16 overflow-y-auto py-1">
          {results.symbols.map((symbol, index) => (
            <button
              key={`${symbol.serverId}:${symbol.workspaceId}:${symbol.name}:${index}`}
              className="flex w-full items-center gap-3 px-4 py-2 text-left hover:bg-accent focus:bg-accent focus:outline-none"
              type="button"
              onClick={() => void openSymbol(symbol, results.generation)}
            >
              <span className="min-w-0 flex-1 truncate text-sm">{symbol.name}</span>
              <span className="max-w-[45%] truncate text-xs text-muted-foreground">
                {symbol.containerName ?? symbol.location?.path ?? symbol.serverId}
              </span>
            </button>
          ))}
          {!loading && results.symbols.length === 0 && showLanguageServerHint ? (
            <LanguageServerHint onOpenSettings={() => setOpen(false)} />
          ) : null}
          {!loading && results.symbols.length === 0 && !showLanguageServerHint ? (
            <p className="px-4 py-5 text-center text-sm text-muted-foreground">No symbols found</p>
          ) : null}
          {loading ? (
            <p className="px-4 py-5 text-center text-sm text-muted-foreground">Searching...</p>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function LanguageServerHint({ onOpenSettings }: { onOpenSettings(): void }) {
  const openSettings = useSettingsDialogStore((state) => state.openSettings);

  return (
    <div className="flex flex-col items-center gap-3 px-4 py-5 text-center text-sm text-muted-foreground">
      <p>
        No language server is running for this workspace. Install one in Settings → Language
        Servers, then open a supported file.
      </p>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => {
          onOpenSettings();
          openSettings(LANGUAGE_SERVERS_SETTINGS_SECTION);
        }}
      >
        Open Language Servers
      </Button>
    </div>
  );
}

function useLanguageServersReady(open: boolean): boolean {
  const initializeLanguageServers = useLanguageServerStore((state) => state.initializeLanguageServers);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    let active = true;
    void initializeLanguageServers().then(() => {
      if (active) {
        setReady(true);
      }
    });
    return () => {
      active = false;
    };
  }, [initializeLanguageServers, open]);

  return ready;
}

function cancellationSource(): { token: RequestCancellation; cancel(): void } {
  let cancelled = false;
  const listeners = new Set<() => void>();
  return {
    token: {
      get isCancellationRequested() {
        return cancelled;
      },
      onCancellationRequested(listener) {
        listeners.add(listener);
        return { dispose: () => listeners.delete(listener) };
      },
    },
    cancel() {
      if (cancelled) {
        return;
      }
      cancelled = true;
      listeners.forEach((listener) => listener());
      listeners.clear();
    },
  };
}
