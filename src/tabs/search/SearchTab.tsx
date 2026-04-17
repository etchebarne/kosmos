import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MagnifyingGlass, File, TextT, ArrowElbowDownLeft, Asterisk } from "@phosphor-icons/react";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { useLayoutStore } from "../../store/layout.store";
import { getFileName, joinPath } from "../../lib/path-utils";
import { revealPosition } from "../editor/editor-cache";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { StateView } from "../../components/shared/StateView";
import { highlightedParts } from "./fuzzy";
import { FilePreview } from "./FilePreview";
import type { TabContentProps } from "../types";

type SearchMode = "files" | "content";

interface FileResult {
  /** Absolute path. */
  path: string;
  /** Bare filename. */
  name: string;
  /** Path relative to the workspace root — what gets matched and highlighted. */
  relative_path: string;
  /** fff combined score (higher is better). */
  score: number;
  /** Byte offsets into `relative_path` that matched the query. */
  indices: number[];
}

interface ContentResult {
  path: string;
  line: number;
  col: number;
  text: string;
}

// ── Main component ──

export function SearchTab({ tab: _tab, paneId }: TabContentProps) {
  const [mode, setMode] = useState<SearchMode>("files");
  const [query, setQuery] = useState("");
  const [fileResults, setFileResults] = useState<FileResult[]>([]);
  const [fileLoading, setFileLoading] = useState(false);
  const [contentResults, setContentResults] = useState<ContentResult[]>([]);
  const [contentLoading, setContentLoading] = useState(false);
  const [useRegex, setUseRegex] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const activeWorkspace = useActiveWorkspace();

  // Auto-focus input on mount
  useEffect(() => {
    setTimeout(() => inputRef.current?.focus(), 0);
  }, []);

  // Point fff at the active workspace so its background indexer + watcher spin up.
  // Idempotent for the same path on the Rust side.
  useEffect(() => {
    if (!activeWorkspace) return;
    invoke("fff_set_workspace", { path: activeWorkspace.path }).catch((e) => {
      console.warn("fff_set_workspace failed:", e);
    });
  }, [activeWorkspace?.path]);

  // Fuzzy file search via fff (debounced)
  useEffect(() => {
    if (mode !== "files") return;
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);

    if (!query.trim() || !activeWorkspace) {
      setFileResults([]);
      setFileLoading(false);
      return;
    }

    setFileLoading(true);
    searchTimerRef.current = setTimeout(() => {
      invoke<FileResult[]>("fff_search_files", {
        path: activeWorkspace.path,
        query: query.trim(),
        maxResults: 50,
      })
        .then((results) => {
          setFileResults(results);
          setFileLoading(false);
          setSelectedIndex(0);
        })
        .catch((e) => {
          console.warn("fff search failed:", e);
          setFileLoading(false);
        });
    }, 80);

    return () => {
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    };
  }, [mode, query, activeWorkspace?.path]);

  // Content search with debounce
  useEffect(() => {
    if (mode !== "content") return;
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);

    if (!query.trim() || !activeWorkspace) {
      setContentResults([]);
      setContentLoading(false);
      return;
    }

    setContentLoading(true);
    searchTimerRef.current = setTimeout(() => {
      invoke<ContentResult[]>("search_in_files", {
        path: activeWorkspace.path,
        query: query.trim(),
        maxResults: 100,
        useRegex,
      })
        .then((results) => {
          setContentResults(results);
          setContentLoading(false);
          setSelectedIndex(0);
        })
        .catch((e) => {
          console.warn("Content search failed:", e);
          setContentLoading(false);
        });
    }, 300);

    return () => {
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    };
  }, [mode, query, activeWorkspace?.path, useRegex]);

  // Reset selected index when results change
  useEffect(() => {
    setSelectedIndex(0);
  }, [fileResults.length, contentResults.length]);

  const resultCount = (mode === "files" ? fileResults : contentResults).length;

  // Currently selected content result for preview
  const selectedContent =
    mode === "content" && contentResults.length > 0 ? contentResults[selectedIndex] : null;

  // Full path for preview
  const previewPath = useMemo(() => {
    if (!selectedContent || !activeWorkspace) return null;
    return joinPath(activeWorkspace.path, selectedContent.path);
  }, [selectedContent, activeWorkspace]);

  // Open file handler
  const openFile = useCallback(
    (filePath: string) => {
      if (!activeWorkspace) return;
      const fullPath = joinPath(activeWorkspace.path, filePath);
      const fileName = getFileName(filePath);
      useLayoutStore.getState().openFile(fullPath, fileName, paneId);
    },
    [activeWorkspace, paneId],
  );

  const handleSelect = useCallback(
    (index: number) => {
      if (mode === "files") {
        const r = fileResults[index];
        if (!r || !activeWorkspace) return;
        // fff already returns an absolute path; just pass it through to the layout store.
        useLayoutStore.getState().openFile(r.path, r.name, paneId);
      } else {
        const r = contentResults[index];
        if (r) {
          openFile(r.path);
          if (!activeWorkspace) return;
          const fullPath = joinPath(activeWorkspace.path, r.path);
          revealPosition(fullPath, { lineNumber: r.line, column: r.col });
        }
      }
    },
    [mode, fileResults, contentResults, openFile, activeWorkspace, paneId],
  );

  // Keyboard navigation
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, resultCount - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        handleSelect(selectedIndex);
      } else if (e.key === "Tab") {
        e.preventDefault();
        setMode((m) => (m === "files" ? "content" : "files"));
        setSelectedIndex(0);
      }
    },
    [resultCount, selectedIndex, handleSelect],
  );

  // Scroll selected item into view
  useEffect(() => {
    const item = listRef.current?.querySelector(`[data-index="${selectedIndex}"]`);
    item?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  if (!activeWorkspace) {
    return <StateView message="No workspace open" />;
  }

  return (
    <div className="flex flex-col h-full font-ui">
      {/* Input area */}
      <div className="flex items-center gap-2 px-3 h-11 shrink-0 border-b border-[var(--color-border-primary)]">
        <MagnifyingGlass size={15} className="text-[var(--color-text-muted)] shrink-0" />
        <input
          ref={inputRef}
          type="text"
          className="flex-1 bg-transparent text-sm text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)] focus:outline-none"
          placeholder={mode === "files" ? "Search files by name..." : "Search in file contents..."}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
        />
        {mode === "content" && (
          <button
            className={`p-1 rounded transition-colors cursor-pointer ${
              useRegex
                ? "text-[var(--color-accent-blue)] bg-[var(--color-bg-input)]"
                : "text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"
            }`}
            onClick={() => setUseRegex((r) => !r)}
            title="Use regular expression"
          >
            <Asterisk size={14} />
          </button>
        )}
        {mode === "content" && contentResults.length > 0 && (
          <span className="text-[11px] text-[var(--color-text-muted)] shrink-0 font-mono">
            {selectedIndex + 1} / {contentResults.length}
          </span>
        )}
      </div>

      {/* Mode tabs */}
      <div className="flex items-center shrink-0 border-b border-[var(--color-border-primary)]">
        <button
          className={`flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-medium transition-colors cursor-pointer ${
            mode === "files"
              ? "text-[var(--color-accent-blue)] border-b-2 border-[var(--color-accent-blue)]"
              : "text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"
          }`}
          onClick={() => {
            setMode("files");
            setSelectedIndex(0);
            inputRef.current?.focus();
          }}
        >
          <File size={12} />
          Files
        </button>
        <button
          className={`flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-medium transition-colors cursor-pointer ${
            mode === "content"
              ? "text-[var(--color-accent-blue)] border-b-2 border-[var(--color-accent-blue)]"
              : "text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"
          }`}
          onClick={() => {
            setMode("content");
            setSelectedIndex(0);
            inputRef.current?.focus();
          }}
        >
          <TextT size={12} />
          Content
        </button>
        <div className="flex-1" />
        <span className="text-[10px] text-[var(--color-text-muted)] px-3">Tab to switch</span>
      </div>

      {/* Results area */}
      {mode === "files" ? (
        /* ── File search: single list ── */
        <ScrollArea className="flex-1 min-h-0">
          <div ref={listRef}>
            {!query.trim() ? (
              <div className="px-4 py-6 text-xs text-[var(--color-text-muted)] text-center">
                Type to search for files...
              </div>
            ) : fileLoading ? (
              <div className="px-4 py-6 text-xs text-[var(--color-text-muted)] text-center">
                Searching...
              </div>
            ) : fileResults.length === 0 ? (
              <div className="px-4 py-6 text-xs text-[var(--color-text-muted)] text-center">
                No files found
              </div>
            ) : (
              <div className="py-1">
                {fileResults.map((r, i) => (
                  <button
                    key={r.path}
                    data-index={i}
                    className={`w-full text-left px-3 py-1.5 flex items-center gap-2.5 cursor-pointer transition-colors ${
                      i === selectedIndex
                        ? "bg-[var(--color-bg-input)]"
                        : "hover:bg-[var(--color-bg-surface)]"
                    }`}
                    onClick={() => handleSelect(i)}
                    onMouseEnter={() => setSelectedIndex(i)}
                  >
                    <File size={14} className="text-[var(--color-text-muted)] shrink-0" />
                    <div className="flex flex-col min-w-0 flex-1">
                      <span className="text-[12px] text-[var(--color-text-primary)] font-medium truncate">
                        {r.name}
                      </span>
                      <span className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                        {highlightedParts(r.relative_path, r.indices).map((p, j) =>
                          p.highlighted ? (
                            <span key={j} className="text-[var(--color-accent-blue)] font-semibold">
                              {p.text}
                            </span>
                          ) : (
                            <span key={j}>{p.text}</span>
                          ),
                        )}
                      </span>
                    </div>
                    {i === selectedIndex && (
                      <ArrowElbowDownLeft
                        size={12}
                        className="text-[var(--color-text-muted)] shrink-0"
                      />
                    )}
                  </button>
                ))}
              </div>
            )}
          </div>
        </ScrollArea>
      ) : (
        /* ── Content search: results list + file preview ── */
        <div className="flex flex-1 min-h-0">
          {/* Results list */}
          <div className="flex flex-col w-1/2 min-w-0 border-r border-[var(--color-border-primary)]">
            <ScrollArea className="flex-1 min-h-0">
              <div ref={listRef}>
                {!query.trim() ? (
                  <div className="px-4 py-6 text-xs text-[var(--color-text-muted)] text-center">
                    Type to search in file contents...
                  </div>
                ) : contentLoading ? (
                  <div className="px-4 py-6 text-xs text-[var(--color-text-muted)] text-center">
                    Searching...
                  </div>
                ) : contentResults.length === 0 ? (
                  <div className="px-4 py-6 text-xs text-[var(--color-text-muted)] text-center">
                    No matches found
                  </div>
                ) : (
                  <div className="py-1">
                    {contentResults.map((r, i) => (
                      <button
                        key={`${r.path}:${r.line}:${r.col}`}
                        data-index={i}
                        className={`w-full text-left px-3 py-1.5 flex items-center gap-2.5 cursor-pointer transition-colors ${
                          i === selectedIndex
                            ? "bg-[var(--color-bg-input)]"
                            : "hover:bg-[var(--color-bg-surface)]"
                        }`}
                        onClick={() => handleSelect(i)}
                        onMouseEnter={() => setSelectedIndex(i)}
                      >
                        <File size={14} className="text-[var(--color-text-muted)] shrink-0" />
                        <div className="flex flex-col min-w-0 flex-1">
                          <div className="flex items-center gap-1.5">
                            <span className="text-[12px] text-[var(--color-text-primary)] font-medium truncate">
                              {getFileName(r.path)}
                            </span>
                            <span className="text-[10px] text-[var(--color-text-muted)]">
                              :{r.line}
                            </span>
                          </div>
                          <span className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                            {r.path}
                          </span>
                        </div>
                        {i === selectedIndex && (
                          <ArrowElbowDownLeft
                            size={12}
                            className="text-[var(--color-text-muted)] shrink-0"
                          />
                        )}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </ScrollArea>
          </div>

          {/* File preview */}
          <div className="flex-1 min-w-0">
            {previewPath && selectedContent ? (
              <FilePreview filePath={previewPath} matchLine={selectedContent.line} query={query} />
            ) : (
              <div className="flex items-center justify-center h-full text-xs text-[var(--color-text-muted)]">
                Select a result to preview
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
