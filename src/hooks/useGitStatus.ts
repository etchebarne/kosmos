import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { GitStatusInfo } from "../lib/gitTree";
import { useWorkspaceWatch } from "./useWorkspaceWatch";

export function useGitStatus(workspacePath: string | null, active = true) {
  const [status, setStatus] = useState<GitStatusInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inflightRef = useRef(false);
  const pendingRef = useRef(false);

  useWorkspaceWatch(workspacePath, active);

  const refresh = useCallback(
    async (silent = false) => {
      if (!workspacePath) return;

      // Collapse concurrent calls to avoid a git process pileup.
      if (inflightRef.current) {
        pendingRef.current = true;
        return;
      }

      inflightRef.current = true;
      if (!silent) setLoading(true);
      setError(null);
      try {
        const result = await invoke<GitStatusInfo>("get_git_status", {
          path: workspacePath,
        });
        setStatus(result);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
        inflightRef.current = false;

        if (pendingRef.current) {
          pendingRef.current = false;
          refresh(true);
        }
      }
    },
    [workspacePath],
  );

  useEffect(() => {
    if (!workspacePath || !active) return;

    refresh();

    const unlisten = listen("git-changed", () => {
      refresh(true);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [workspacePath, refresh, active]);

  return { status, loading, error, setError, refresh };
}
