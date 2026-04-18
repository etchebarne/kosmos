import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Trash, CaretRight, File } from "@phosphor-icons/react";
import { Dialog } from "../../components/shared/Dialog";
import { gitStatusColor, gitStatusLabel } from "../../lib/gitColors";

interface GitStashEntry {
  index: number;
  message: string;
}

interface GitStashFile {
  path: string;
  status: string;
}

interface StashDialogProps {
  open: boolean;
  onClose: () => void;
  workspacePath: string;
  onApply: () => void;
}

function StashEntryRow({
  entry,
  workspacePath,
  onPop,
  onDrop,
}: {
  entry: GitStashEntry;
  workspacePath: string;
  onPop: (index: number) => void;
  onDrop: (index: number) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [files, setFiles] = useState<GitStashFile[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);

  const loadFiles = useCallback(async () => {
    if (files.length > 0) return;
    setFilesLoading(true);
    try {
      const result = await invoke<GitStashFile[]>("git_stash_show", {
        path: workspacePath,
        index: entry.index,
      });
      setFiles(result);
    } catch {
      // User can retry by toggling expand.
    } finally {
      setFilesLoading(false);
    }
  }, [workspacePath, entry.index, files.length]);

  const handleToggle = () => {
    const next = !expanded;
    setExpanded(next);
    if (next) loadFiles();
  };

  return (
    <div className="border-b border-[var(--color-border-primary)] last:border-b-0">
      <div className="flex items-center gap-2 px-4 py-2 hover:bg-[var(--color-bg-elevated)] transition-colors">
        <button
          className="shrink-0 w-4 h-4 flex items-center justify-center text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer"
          onClick={handleToggle}
        >
          <span
            className={`flex items-center justify-center transition-transform duration-200 ${expanded ? "rotate-90" : ""}`}
          >
            <CaretRight size={12} />
          </span>
        </button>
        <span
          className="text-xs text-[var(--color-text-primary)] flex-1 truncate cursor-pointer"
          onClick={handleToggle}
        >
          {entry.message}
        </span>
        <button
          className="shrink-0 px-2 py-0.5 text-[11px] text-[var(--color-accent-blue)] hover:text-[var(--color-accent-blue-hover)] hover:bg-[var(--color-bg-hover)] transition-colors cursor-pointer rounded"
          onClick={() => onPop(entry.index)}
          title="Apply and remove this stash"
        >
          Apply
        </button>
        <button
          className="shrink-0 p-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-status-red)] hover:bg-[var(--color-bg-hover)] transition-colors cursor-pointer rounded"
          onClick={() => onDrop(entry.index)}
          title="Delete this stash"
        >
          <Trash size={12} />
        </button>
      </div>
      {expanded && (
        <div className="bg-[var(--color-bg-surface)]">
          {filesLoading && (
            <div className="px-8 py-2">
              <span className="text-[11px] text-[var(--color-text-muted)]">Loading...</span>
            </div>
          )}
          {!filesLoading && files.length === 0 && (
            <div className="px-8 py-2">
              <span className="text-[11px] text-[var(--color-text-muted)]">No files</span>
            </div>
          )}
          {!filesLoading &&
            files.map((file) => (
              <div key={file.path} className="flex items-center gap-2 px-8 py-1.5">
                <File size={12} className={gitStatusColor(file.status)} />
                <span className="text-[11px] text-[var(--color-text-secondary)] flex-1 truncate">
                  {file.path}
                </span>
                <span
                  className={`text-[10px] font-mono font-medium ${gitStatusColor(file.status)}`}
                >
                  {gitStatusLabel(file.status)}
                </span>
              </div>
            ))}
        </div>
      )}
    </div>
  );
}

export function StashDialog({ open, onClose, workspacePath, onApply }: StashDialogProps) {
  const [entries, setEntries] = useState<GitStashEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchStashes = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<GitStashEntry[]>("git_stash_list", {
        path: workspacePath,
      });
      setEntries(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    if (open) fetchStashes();
  }, [open, fetchStashes]);

  const handlePop = useCallback(
    async (index: number) => {
      try {
        await invoke("git_stash_pop", { path: workspacePath, index });
        onApply();
        fetchStashes();
      } catch (e) {
        setError(String(e));
      }
    },
    [workspacePath, onApply, fetchStashes],
  );

  const handleDrop = useCallback(
    async (index: number) => {
      try {
        await invoke("git_stash_drop", { path: workspacePath, index });
        fetchStashes();
      } catch (e) {
        setError(String(e));
      }
    },
    [workspacePath, fetchStashes],
  );

  return (
    <Dialog open={open} onClose={onClose} title="Stash">
      <div className="font-ui">
        {loading && (
          <div className="px-4 py-6 text-center">
            <span className="text-xs text-[var(--color-text-secondary)]">Loading...</span>
          </div>
        )}
        {error && (
          <div className="px-4 py-3">
            <span className="text-xs text-[var(--color-status-red)]">{error}</span>
          </div>
        )}
        {!loading && !error && entries.length === 0 && (
          <div className="px-4 py-6 text-center">
            <span className="text-xs text-[var(--color-text-muted)]">No stashes</span>
          </div>
        )}
        {!loading &&
          entries.map((entry) => (
            <StashEntryRow
              key={entry.index}
              entry={entry}
              workspacePath={workspacePath}
              onPop={handlePop}
              onDrop={handleDrop}
            />
          ))}
      </div>
    </Dialog>
  );
}
