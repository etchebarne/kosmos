import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getNodeFiles } from "../../lib/gitTree";
import type { TreeNode, GitStatusInfo } from "../../lib/gitTree";

export function useGitActions(
  workspacePath: string | null,
  status: GitStatusInfo | null,
  commitMessage: string,
  setCommitMessage: (msg: string) => void,
  setCommitting: (v: boolean) => void,
  refresh: () => void,
  setError: (error: string | null) => void,
) {
  const handleStageAll = useCallback(async () => {
    if (!workspacePath) return;
    try {
      await invoke("git_stage_all", { path: workspacePath });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [workspacePath, refresh, setError]);

  const handleUnstageAll = useCallback(async () => {
    if (!workspacePath || !status) return;
    try {
      const stagedFiles = status.changes.filter((c) => c.staged).map((c) => c.path);
      if (stagedFiles.length > 0) {
        await invoke("git_unstage", {
          path: workspacePath,
          files: stagedFiles,
        });
        refresh();
      }
    } catch (e) {
      setError(String(e));
    }
  }, [workspacePath, status, refresh, setError]);

  const handleToggleStage = useCallback(
    async (node: TreeNode) => {
      if (!workspacePath) return;
      const files = getNodeFiles(node).map((f) => f.path);
      const allStaged = getNodeFiles(node).every((f) => f.staged);
      try {
        if (allStaged) {
          await invoke("git_unstage", {
            path: workspacePath,
            files,
          });
        } else {
          await invoke("git_stage", { path: workspacePath, files });
        }
        refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [workspacePath, refresh, setError],
  );

  const handleCommit = useCallback(async () => {
    if (!workspacePath || !commitMessage.trim()) return;
    setCommitting(true);
    try {
      await invoke("git_commit", {
        path: workspacePath,
        message: commitMessage,
      });
      setCommitMessage("");
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setCommitting(false);
    }
  }, [workspacePath, commitMessage, refresh, setError, setCommitting, setCommitMessage]);

  const handleInit = useCallback(async () => {
    if (!workspacePath) return;
    try {
      await invoke("git_init", { path: workspacePath });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [workspacePath, refresh, setError]);

  const handleStashAll = useCallback(async () => {
    if (!workspacePath) return;
    try {
      await invoke("git_stash_all", { path: workspacePath });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [workspacePath, refresh, setError]);

  const handleStashFiles = useCallback(
    async (node: TreeNode) => {
      if (!workspacePath) return;
      const files = getNodeFiles(node).map((f) => f.path);
      try {
        await invoke("git_stash_files", { path: workspacePath, files });
        refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [workspacePath, refresh, setError],
  );

  const handleDiscardAllTracked = useCallback(async () => {
    if (!workspacePath) return;
    try {
      await invoke("git_discard_all_tracked", { path: workspacePath });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [workspacePath, refresh, setError]);

  const handleTrashAllUntracked = useCallback(async () => {
    if (!workspacePath) return;
    try {
      await invoke("git_trash_all_untracked", { path: workspacePath });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [workspacePath, refresh, setError]);

  const handleDiscard = useCallback(
    async (node: TreeNode) => {
      if (!workspacePath) return;
      const files = getNodeFiles(node).map((f) => f.path);
      try {
        await invoke("git_discard", { path: workspacePath, files });
        refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [workspacePath, refresh, setError],
  );

  const handleTrash = useCallback(
    async (node: TreeNode) => {
      if (!workspacePath) return;
      const files = getNodeFiles(node).map((f) => f.path);
      try {
        await invoke("git_trash_untracked", { path: workspacePath, files });
        refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [workspacePath, refresh, setError],
  );

  return {
    handleStageAll,
    handleUnstageAll,
    handleToggleStage,
    handleCommit,
    handleInit,
    handleStashAll,
    handleStashFiles,
    handleDiscardAllTracked,
    handleTrashAllUntracked,
    handleDiscard,
    handleTrash,
  };
}
