import { FileSearch, LoaderCircle, Search as SearchIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { FileIcon } from "@/renderer/components/file-icon";
import { Button } from "@/renderer/components/ui/button";
import { ButtonGroup } from "@/renderer/components/ui/button-group";
import { Input } from "@/renderer/components/ui/input";
import { getSearchDocument, searchWorkspace } from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import { applyMonacoTheme, monaco } from "@/renderer/lib/monaco";
import { useWorkspaceStore } from "@/renderer/stores";
import type {
  SearchDocument,
  SearchMatch,
  SearchMode,
  TabId,
  WorkspaceId,
  WorkspaceSearchResults,
} from "@/shared/ipc";

type SearchTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  isActive: boolean;
  onActivatePane(): void;
};

type SearchState = {
  status: "idle" | "searching" | "loaded" | "error";
  results: WorkspaceSearchResults;
  message?: string;
};

type PendingSearch = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  query: string;
  mode: SearchMode;
};

type PreviewState =
  | { status: "idle" | "loading" }
  | { status: "loaded"; document: SearchDocument }
  | { status: "error"; message: string };

const SEARCH_DELAY_MS = 220;
const EMPTY_RESULTS: WorkspaceSearchResults = { matches: [], limitReached: false };

export function SearchTab({ workspaceId, tabId, isActive, onActivatePane }: SearchTabProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const inFlightRef = useRef(false);
  const pendingRef = useRef<PendingSearch | null>(null);
  const desiredRef = useRef<PendingSearch>({ workspaceId, tabId, query: "", mode: "name" });
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<SearchMode>("name");
  const [searchState, setSearchState] = useState<SearchState>({
    status: "idle",
    results: EMPTY_RESULTS,
  });
  const [selectedIndex, setSelectedIndex] = useState(0);

  desiredRef.current = { workspaceId, tabId, query: query.trim(), mode };

  const runSearch = async (request: PendingSearch) => {
    if (inFlightRef.current) {
      pendingRef.current = request;
      return;
    }

    inFlightRef.current = true;
    setSearchState((current) => ({ ...current, status: "searching", message: undefined }));
    try {
      const results = await searchWorkspace(request);
      if (sameSearch(desiredRef.current, request)) {
        setSearchState({ status: "loaded", results });
        setSelectedIndex(0);
      }
    } catch (caughtError: unknown) {
      if (sameSearch(desiredRef.current, request)) {
        setSearchState((current) => ({
          ...current,
          status: "error",
          message: errorMessage(caughtError),
        }));
      }
    } finally {
      inFlightRef.current = false;
      const pending = pendingRef.current;
      pendingRef.current = null;
      if (pending && sameSearch(desiredRef.current, pending)) {
        void runSearch(pending);
      }
    }
  };

  useEffect(() => {
    const request = desiredRef.current;
    if (!request.query) {
      pendingRef.current = null;
      setSearchState({ status: "idle", results: EMPTY_RESULTS });
      setSelectedIndex(0);
      return undefined;
    }

    const timeout = window.setTimeout(() => void runSearch(request), SEARCH_DELAY_MS);
    return () => window.clearTimeout(timeout);
  }, [workspaceId, tabId, query, mode]);

  useEffect(() => {
    if (isActive) {
      inputRef.current?.focus({ preventScroll: true });
    }
  }, [isActive]);

  const selectedMatch = searchState.results.matches[selectedIndex] ?? null;
  const selectMode = (nextMode: SearchMode) => {
    if (nextMode === mode) {
      return;
    }

    setMode(nextMode);
    setSearchState({ status: query.trim() ? "searching" : "idle", results: EMPTY_RESULTS });
    setSelectedIndex(0);
  };

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background"
      onPointerDown={onActivatePane}
    >
      <div className="flex shrink-0 flex-col gap-2 p-3 sm:flex-row sm:items-center">
        <div className="relative min-w-0 flex-1">
          <SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={inputRef}
            value={query}
            maxLength={256}
            spellCheck={false}
            aria-label="Search workspace"
            placeholder={mode === "name" ? "Search file names" : "Search file contents"}
            className="pl-8 pr-8"
            onChange={(event) => setQuery(event.target.value)}
          />
          {searchState.status === "searching" ? (
            <LoaderCircle className="pointer-events-none absolute right-2.5 top-1/2 size-4 -translate-y-1/2 animate-spin text-muted-foreground" />
          ) : null}
        </div>
        <ButtonGroup className="shrink-0 self-end sm:self-auto" aria-label="Search mode">
          <Button
            type="button"
            variant={mode === "name" ? "secondary" : "outline"}
            aria-pressed={mode === "name"}
            onClick={() => selectMode("name")}
          >
            Name
          </Button>
          <Button
            type="button"
            variant={mode === "content" ? "secondary" : "outline"}
            aria-pressed={mode === "content"}
            onClick={() => selectMode("content")}
          >
            Content
          </Button>
        </ButtonGroup>
      </div>

      {!query.trim() ? <SearchMessage message="Search this workspace by file name or content." /> : null}
      {query.trim() && searchState.status === "error" ? (
        <SearchMessage message={searchState.message ?? "Search failed."} />
      ) : null}
      {query.trim() && searchState.status !== "error" && searchState.results.matches.length === 0 ? (
        <SearchMessage
          message={searchState.status === "searching" ? "Searching..." : "No results found."}
        />
      ) : null}
      {searchState.results.matches.length > 0 ? (
        <div
          className={
            mode === "content"
              ? "grid min-h-0 min-w-0 flex-1 grid-rows-[minmax(10rem,40%)_minmax(0,1fr)] md:grid-cols-[minmax(15rem,35%)_minmax(0,1fr)] md:grid-rows-1"
              : "flex min-h-0 min-w-0 flex-1"
          }
        >
          <SearchResults
            mode={mode}
            matches={searchState.results.matches}
            selectedIndex={selectedIndex}
            onSelect={setSelectedIndex}
            onOpen={(path) => useWorkspaceStore.getState().openEditorTab(tabId, path)}
          />
          {mode === "content" && selectedMatch ? (
            <SearchPreview
              workspaceId={workspaceId}
              tabId={tabId}
              searchMatch={selectedMatch}
              query={query.trim()}
            />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function SearchResults({
  mode,
  matches,
  selectedIndex,
  onSelect,
  onOpen,
}: {
  mode: SearchMode;
  matches: SearchMatch[];
  selectedIndex: number;
  onSelect(index: number): void;
  onOpen(path: string): void;
}) {
  return (
    <div
      className={
        mode === "content"
          ? "flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden border-b border-border md:border-b-0 md:border-r"
          : "flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
      }
    >
      <div className="scrollbar-themed min-h-0 flex-1 overflow-y-auto p-1" role="listbox">
        {matches.map((searchMatch, index) => {
          const fileName = searchMatch.path.split("/").at(-1) ?? searchMatch.path;
          const directory = searchMatch.path.slice(0, -(fileName.length + 1));
          return (
            <Button
              key={`${searchMatch.path}:${searchMatch.lineNumber ?? "name"}:${index}`}
              type="button"
              variant="ghost"
              role="option"
              aria-selected={mode === "content" && index === selectedIndex}
              className="h-auto w-full justify-start gap-2 rounded-md px-2 py-2 text-left font-normal aria-selected:bg-muted"
              onClick={() => (mode === "content" ? onSelect(index) : onOpen(searchMatch.path))}
              onDoubleClick={() => {
                if (mode === "content") {
                  onOpen(searchMatch.path);
                }
              }}
            >
              <FileIcon path={searchMatch.path} className="mt-0.5 size-4 self-start" />
              <span className="min-w-0 flex-1">
                <span className="flex min-w-0 items-baseline gap-2">
                  <span className="truncate text-sm font-medium text-foreground">{fileName}</span>
                  {searchMatch.lineNumber ? (
                    <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                      {searchMatch.lineNumber}
                    </span>
                  ) : null}
                </span>
                <span className="block truncate text-xs text-muted-foreground">
                  {directory || searchMatch.path}
                </span>
                {searchMatch.preview ? (
                  <span className="mt-1 block truncate font-mono text-xs text-muted-foreground">
                    {searchMatch.preview}
                  </span>
                ) : null}
              </span>
            </Button>
          );
        })}
      </div>
    </div>
  );
}

function SearchPreview({
  workspaceId,
  tabId,
  searchMatch,
  query,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  searchMatch: SearchMatch;
  query: string;
}) {
  const [previewState, setPreviewState] = useState<PreviewState>({ status: "loading" });
  const requestIdRef = useRef(0);

  useEffect(() => {
    const requestId = ++requestIdRef.current;
    setPreviewState({ status: "loading" });
    void getSearchDocument({ workspaceId, tabId, path: searchMatch.path })
      .then((document) => {
        if (requestIdRef.current === requestId) {
          setPreviewState({ status: "loaded", document });
        }
      })
      .catch((caughtError: unknown) => {
        if (requestIdRef.current === requestId) {
          setPreviewState({ status: "error", message: errorMessage(caughtError) });
        }
      });
  }, [workspaceId, tabId, searchMatch.path]);

  return (
    <div className="flex min-h-0 min-w-0 flex-col overflow-hidden">
      {previewState.status === "loading" ? <SearchMessage message="Loading preview..." /> : null}
      {previewState.status === "error" ? <SearchMessage message={previewState.message} /> : null}
      {previewState.status === "loaded" ? (
        <MonacoSearchPreview
          workspaceId={workspaceId}
          tabId={tabId}
          document={previewState.document}
          lineNumber={searchMatch.lineNumber ?? 1}
          query={query}
        />
      ) : null}
    </div>
  );
}

function MonacoSearchPreview({
  workspaceId,
  tabId,
  document,
  lineNumber,
  query,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  document: SearchDocument;
  lineNumber: number;
  query: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return undefined;
    }

    applyMonacoTheme();
    const model = monaco.editor.createModel(
      document.content,
      undefined,
      monaco.Uri.from({
        scheme: "kosmos-search",
        authority: `workspace-${workspaceId}`,
        path: `/tab-${tabId}/${document.path}`,
      }),
    );
    const editor = monaco.editor.create(container, {
      model,
      automaticLayout: true,
      bracketPairColorization: { enabled: true },
      contextmenu: false,
      fontSize: 13,
      minimap: { enabled: false },
      padding: { top: 8 },
      readOnly: true,
      domReadOnly: true,
      scrollBeyondLastLine: false,
      smoothScrolling: true,
      theme: "kosmos",
    });
    const frameId = requestAnimationFrame(() => {
      editor.layout();
      const targetLine = Math.min(lineNumber, model.getLineCount());
      const match = model
        .findMatches(query, false, false, false, null, false)
        .find((candidate) => candidate.range.startLineNumber === targetLine);

      if (match) {
        editor.setSelection(match.range);
        editor.revealRangeInCenter(match.range, monaco.editor.ScrollType.Immediate);
      } else {
        const position = { lineNumber: targetLine, column: 1 };
        editor.setPosition(position);
        editor.revealPositionInCenter(position, monaco.editor.ScrollType.Immediate);
      }
    });

    return () => {
      cancelAnimationFrame(frameId);
      editor.dispose();
      model.dispose();
    };
  }, [workspaceId, tabId, document, lineNumber, query]);

  return <div ref={containerRef} className="min-h-0 min-w-0 flex-1" />;
}

function SearchMessage({ message }: { message: string }) {
  return (
    <div className="grid min-h-0 flex-1 place-items-center p-6 text-center">
      <div className="flex max-w-sm flex-col items-center gap-2 text-muted-foreground">
        <FileSearch className="size-6" />
        <p className="text-sm">{message}</p>
      </div>
    </div>
  );
}

function sameSearch(left: PendingSearch, right: PendingSearch): boolean {
  return (
    left.workspaceId === right.workspaceId &&
    left.tabId === right.tabId &&
    left.query === right.query &&
    left.mode === right.mode
  );
}
