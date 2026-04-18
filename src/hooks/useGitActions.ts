import { useState, useMemo, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useToastStore } from "../store/toast.store";

type GitAction = "fetch" | "pull" | "pull_rebase" | "push" | "force_push";

const GIT_ACTIONS: { key: GitAction; label: string; command: string }[] = [
  { key: "fetch", label: "Fetch", command: "git_fetch" },
  { key: "pull", label: "Pull", command: "git_pull" },
  { key: "pull_rebase", label: "Pull (Rebase)", command: "git_pull_rebase" },
  { key: "push", label: "Push", command: "git_push" },
  { key: "force_push", label: "Force Push", command: "git_force_push" },
];

export { GIT_ACTIONS };

export function useGitActions(
  workspacePath: string | null,
  refresh: () => void,
  setError: (error: string | null) => void,
) {
  const [activeAction, setActiveAction] = useState<GitAction>("fetch");
  const [actionRunning, setActionRunning] = useState(false);
  const [actionDone, setActionDone] = useState(false);
  const runningRef = useRef(false);
  const activeRef = useRef<GitAction>("fetch");

  const currentAction = useMemo(
    () => GIT_ACTIONS.find((a) => a.key === activeAction)!,
    [activeAction],
  );

  const handleRunAction = useCallback(
    async (action?: GitAction) => {
      if (!workspacePath || runningRef.current) return;
      const act = GIT_ACTIONS.find((a) => a.key === (action ?? activeRef.current))!;
      if (action) {
        setActiveAction(action);
        activeRef.current = action;
      }
      runningRef.current = true;
      setActionRunning(true);
      try {
        await invoke(act.command, { path: workspacePath });
        refresh();
        setActionDone(true);
        setTimeout(() => setActionDone(false), 2000);
        useToastStore.getState().addToast({
          message: `${act.label} completed successfully`,
          type: "success",
          duration: 4000,
        });
      } catch (e) {
        const msg = String(e);
        setError(msg);
        useToastStore.getState().addToast({
          message: `${act.label} failed: ${msg}`,
          type: "error",
        });
      } finally {
        runningRef.current = false;
        setActionRunning(false);
      }
    },
    [workspacePath, refresh, setError],
  );

  return {
    activeAction,
    setActiveAction,
    actionRunning,
    actionDone,
    currentAction,
    handleRunAction,
  };
}
