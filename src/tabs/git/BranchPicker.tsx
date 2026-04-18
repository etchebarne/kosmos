import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { GitBranch, Trash } from "@phosphor-icons/react";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { useClickOutside } from "../../hooks/useClickOutside";
import { HighlightedText } from "../search/fuzzy";

interface GitBranchInfo {
  name: string;
  isRemote: boolean;
  isCurrent: boolean;
  lastCommitDate: string | null;
}

interface FuzzyHit {
  text: string;
  score: number;
  indices: number[];
}

interface BranchPickerProps {
  workspacePath: string;
  onClose: () => void;
  onSwitch: () => void;
  anchorRef: React.RefObject<HTMLElement | null>;
  ignoreRef?: React.RefObject<HTMLElement | null>;
}

export function BranchPicker({
  workspacePath,
  onClose,
  onSwitch,
  anchorRef,
  ignoreRef,
}: BranchPickerProps) {
  const [branches, setBranches] = useState<GitBranchInfo[]>([]);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [switching, setSwitching] = useState<string | null>(null);
  const [matchIndices, setMatchIndices] = useState<Map<string, number[]>>(new Map());
  const [filtered, setFiltered] = useState<GitBranchInfo[]>([]);
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<GitBranchInfo[]>("git_list_branches", { path: workspacePath })
      .then(setBranches)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [workspacePath]);

  // Ignore clicks on the anchor; its onClick already toggles close (avoids reopen race).
  useClickOutside(ref, onClose, true, ignoreRef);

  useEffect(() => {
    const handle = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handle);
    return () => document.removeEventListener("keydown", handle);
  }, [onClose]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleCheckout = useCallback(
    async (branch: GitBranchInfo) => {
      if (branch.isCurrent) return;
      // Strip the "origin/" prefix so checkout creates a tracking branch.
      const branchName = branch.isRemote ? branch.name.replace(/^[^/]+\//, "") : branch.name;
      setSwitching(branch.name);
      try {
        await invoke("git_checkout", {
          path: workspacePath,
          branch: branchName,
        });
        onSwitch();
        onClose();
      } catch (e) {
        console.error(e);
        setSwitching(null);
      }
    },
    [workspacePath, onSwitch, onClose],
  );

  const handleDelete = useCallback(
    async (branch: GitBranchInfo) => {
      try {
        await invoke("git_delete_branch", {
          path: workspacePath,
          branch: branch.name,
        });
        setBranches((prev) => prev.filter((b) => b.name !== branch.name));
        onSwitch();
      } catch (e) {
        console.error(e);
      }
    },
    [workspacePath, onSwitch],
  );

  // Route through the Rust fuzzy matcher to match the file picker's ranking.
  useEffect(() => {
    let cancelled = false;
    const trimmed = search.trim();

    if (!trimmed) {
      setFiltered(branches);
      setMatchIndices(new Map());
      return;
    }

    invoke<FuzzyHit[]>("fuzzy_match", {
      query: trimmed,
      items: branches.map((b) => b.name),
      mode: "plain",
    })
      .then((hits) => {
        if (cancelled) return;
        const byName = new Map(branches.map((b) => [b.name, b] as const));
        const ordered = hits.map((h) => byName.get(h.text)).filter((b): b is GitBranchInfo => !!b);
        setFiltered(ordered);
        setMatchIndices(new Map(hits.map((h) => [h.text, h.indices])));
      })
      .catch((e) => {
        if (cancelled) return;
        console.warn("branch fuzzy_match failed:", e);
        // Substring fallback keeps the picker usable if the matcher fails.
        const q = trimmed.toLowerCase();
        setFiltered(branches.filter((b) => b.name.toLowerCase().includes(q)));
        setMatchIndices(new Map());
      });

    return () => {
      cancelled = true;
    };
  }, [search, branches]);

  const position = useMemo(() => {
    const rect = anchorRef.current?.getBoundingClientRect();
    if (!rect) return { bottom: 0, left: 0, width: 0 };
    return {
      bottom: window.innerHeight - rect.top,
      left: rect.left,
      width: Math.max(rect.width, 260),
    };
  }, [anchorRef]);

  return (
    <div
      ref={ref}
      className="fixed z-50 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] shadow-lg flex flex-col rounded-md overflow-hidden"
      style={{
        bottom: position.bottom,
        left: position.left,
        width: position.width,
        maxHeight: 400,
      }}
    >
      {/* Header */}
      <div className="px-3 py-2 border-b border-[var(--color-border-primary)] bg-[var(--color-bg-surface)]">
        <span className="text-[11px] font-medium uppercase tracking-wider text-[var(--color-text-secondary)]">
          Branches
        </span>
      </div>

      {/* Branch list */}
      <ScrollArea className="flex-1 min-h-0">
        {loading ? (
          <div className="px-3 py-4 text-xs text-[var(--color-text-muted)]">Loading...</div>
        ) : filtered.length === 0 ? (
          <div className="px-3 py-4 text-xs text-[var(--color-text-muted)]">No branches found</div>
        ) : (
          <div className="py-1">
            {filtered.map((branch) => (
              <div
                key={branch.name}
                className={`group flex items-center hover:bg-[var(--color-bg-input)] transition-colors border-l-2 ${
                  branch.isCurrent
                    ? "border-l-[var(--color-accent-blue)] bg-[var(--color-bg-surface)]"
                    : "border-l-transparent"
                }`}
              >
                <button
                  className="flex-1 min-w-0 text-left px-3 py-2 flex items-center gap-2.5 cursor-pointer outline-none focus:bg-[var(--color-bg-input)] rounded-none"
                  onClick={() => handleCheckout(branch)}
                  disabled={switching !== null}
                >
                  <GitBranch
                    size={14}
                    className={`shrink-0 ${
                      branch.isCurrent
                        ? "text-[var(--color-accent-blue)]"
                        : branch.isRemote
                          ? "text-[var(--color-text-muted)]"
                          : "text-[var(--color-text-tertiary)]"
                    }`}
                  />
                  <div className="flex flex-col min-w-0 flex-1">
                    <span
                      className={`text-[12px] truncate ${
                        branch.isCurrent
                          ? "text-[var(--color-accent-blue)]"
                          : "text-[var(--color-text-primary)]"
                      }`}
                    >
                      <HighlightedText
                        text={branch.name}
                        indices={matchIndices.get(branch.name) ?? []}
                      />
                    </span>
                    {branch.lastCommitDate && (
                      <span className="text-[10px] text-[var(--color-text-tertiary)] mt-0.5">
                        {branch.lastCommitDate}
                      </span>
                    )}
                  </div>
                  {switching === branch.name && (
                    <span className="text-[10px] text-[var(--color-text-muted)] shrink-0">...</span>
                  )}
                </button>
                {!branch.isCurrent && !branch.isRemote && (
                  <button
                    className="shrink-0 p-2 mr-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-status-red)] transition-colors cursor-pointer rounded-none"
                    onClick={() => handleDelete(branch)}
                    title={`Delete ${branch.name}`}
                  >
                    <Trash size={14} />
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </ScrollArea>

      {/* Search */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-t border-[var(--color-border-primary)]">
        <input
          ref={inputRef}
          type="text"
          className="flex-1 bg-transparent text-xs text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)] focus:outline-none"
          placeholder="Select branch or remote..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
    </div>
  );
}
