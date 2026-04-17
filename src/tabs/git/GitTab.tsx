import { useState, useCallback, useRef, type MouseEvent } from "react";
import {
  GitBranch,
  CaretDown,
  ArrowsClockwise,
  Check,
  CircleNotch,
  DotsThree,
  ArrowUp,
  ArrowDown,
} from "@phosphor-icons/react";
import { useActiveWorkspace, useIsWorkspaceActive } from "../../contexts/WorkspaceContext";
import { GitChangeNode } from "./GitChangeNode";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { StateView } from "../../components/shared/StateView";
import { ContextMenu } from "../../components/shared/ContextMenu";
import type { ContextMenuItem } from "../../components/shared/ContextMenu";
import { BranchPicker } from "./BranchPicker";
import { StashDialog } from "./StashDialog";
import { useGitStatus } from "../../hooks/use-git-status";
import { useGitActions as useGitRemoteActions, GIT_ACTIONS } from "../../hooks/use-git-actions";
import { buildChangeTree, getNodeFiles } from "../../lib/git-tree";
import type { TreeNode } from "../../lib/git-tree";
import { useLayoutStore } from "../../store/layout.store";
import { useGitActions } from "./useGitActions";
import type { TabContentProps } from "../types";

export function GitTab({ tab: _tab, paneId }: TabContentProps) {
  const activeWorkspace = useActiveWorkspace();
  const isActive = useIsWorkspaceActive();
  const workspacePath = activeWorkspace?.path ?? null;

  const { status, loading, error, setError, refresh } = useGitStatus(workspacePath, isActive);
  const { activeAction, actionRunning, actionDone, currentAction, handleRunAction } =
    useGitRemoteActions(workspacePath, refresh, setError);

  const [commitMessage, setCommitMessage] = useState("");
  const [committing, setCommitting] = useState(false);
  const [showBranchPicker, setShowBranchPicker] = useState(false);
  const [showStashDialog, setShowStashDialog] = useState(false);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    items: ContextMenuItem[];
    placement?: "top-start" | "top-end" | "bottom-start" | "bottom-end";
  } | null>(null);
  const branchBarRef = useRef<HTMLDivElement>(null);
  const branchButtonRef = useRef<HTMLButtonElement>(null);

  const {
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
  } = useGitActions(
    workspacePath,
    status,
    commitMessage,
    setCommitMessage,
    setCommitting,
    refresh,
    setError,
  );

  const handleFileClick = useCallback(
    (node: TreeNode) => {
      if (node.isDir || !node.change) return;
      const fileName = node.name;
      const isUntracked = node.change.status === "untracked";
      useLayoutStore
        .getState()
        .openChanges(node.change.path, fileName, node.change.staged, isUntracked, paneId);
    },
    [paneId],
  );

  const handleNodeContextMenu = useCallback(
    (e: MouseEvent, node: TreeNode) => {
      e.preventDefault();
      const files = getNodeFiles(node);
      const isUntracked = files.every((f) => f.status === "untracked");
      const items: ContextMenuItem[] = isUntracked
        ? [{ label: "Trash", onClick: () => handleTrash(node) }]
        : [
            { label: "Stash", onClick: () => handleStashFiles(node) },
            { label: "Discard Changes", onClick: () => handleDiscard(node) },
          ];
      setContextMenu({ x: e.clientX, y: e.clientY, items });
    },
    [handleDiscard, handleTrash, handleStashFiles],
  );

  if (!activeWorkspace) {
    return <StateView message="No workspace open" />;
  }

  if (loading && !status) {
    return <StateView message="Loading..." variant="secondary" />;
  }

  if (error && !status) {
    return <StateView message={error} variant="error" />;
  }

  if (status && !status.isRepo) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 font-ui">
        <p className="text-xs text-[var(--color-text-muted)]">
          This workspace is not a git repository
        </p>
        <button
          className="px-3 py-1.5 text-xs text-[var(--color-text-primary)] bg-[var(--color-bg-elevated)] border border-[var(--color-border-secondary)] hover:bg-[var(--color-bg-surface)] transition-colors cursor-pointer rounded-none"
          onClick={handleInit}
        >
          Initialize Git
        </button>
      </div>
    );
  }

  const changes = status?.changes ?? [];
  const tracked = changes.filter((c) => c.status !== "untracked");
  const untracked = changes.filter((c) => c.status === "untracked");
  const trackedTree = buildChangeTree(tracked);
  const untrackedTree = buildChangeTree(untracked);
  const stagedCount = changes.filter((c) => c.staged).length;
  const allStaged = changes.length > 0 && stagedCount === changes.length;

  return (
    <div className="flex flex-col h-full font-ui">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--color-border-primary)]">
        <div className="flex items-center gap-1">
          <button
            className="p-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer disabled:opacity-50"
            onClick={() => refresh()}
            disabled={loading}
            title="Refresh"
          >
            {loading ? (
              <CircleNotch size={14} className="animate-spin" />
            ) : (
              <ArrowsClockwise size={14} />
            )}
          </button>
          <span className="text-xs text-[var(--color-text-primary)]">
            {changes.length === 0
              ? "No Changes"
              : `${changes.length} Change${changes.length !== 1 ? "s" : ""}`}
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            className="text-xs text-[var(--color-accent-blue)] hover:text-[var(--color-accent-blue-hover)] transition-colors cursor-pointer"
            onClick={allStaged ? handleUnstageAll : handleStageAll}
          >
            {allStaged ? "Unstage All" : "Stage All"}
          </button>
          <button
            className="p-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer"
            onClick={(e) => {
              const rect = (e.currentTarget as HTMLButtonElement).getBoundingClientRect();
              setContextMenu({
                x: rect.right,
                y: rect.bottom + 4,
                items: [
                  { label: "Stash All", onClick: handleStashAll },
                  { label: "View Stash", onClick: () => setShowStashDialog(true) },
                  { separator: true },
                  { label: "Discard Tracked Changes", onClick: handleDiscardAllTracked },
                  { label: "Trash Untracked Files", onClick: handleTrashAllUntracked },
                ],
              });
            }}
          >
            <DotsThree size={14} />
          </button>
        </div>
      </div>

      {/* Changes tree */}
      <ScrollArea className="flex-1">
        {changes.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <p className="text-xs text-[var(--color-text-muted)]">No changes</p>
          </div>
        ) : (
          <div className="pt-1 pb-4">
            {tracked.length > 0 && (
              <>
                <div className="px-3 py-1">
                  <span className="text-[11px] font-medium text-[var(--color-text-tertiary)] uppercase tracking-wider">
                    Tracked
                  </span>
                </div>
                {trackedTree.map((node) => (
                  <GitChangeNode
                    key={node.path}
                    node={node}
                    depth={0}
                    isUntracked={false}
                    onToggleStage={handleToggleStage}
                    onContextMenu={handleNodeContextMenu}
                    onFileClick={handleFileClick}
                  />
                ))}
              </>
            )}
            {untracked.length > 0 && (
              <>
                <div className="px-3 py-1 mt-2">
                  <span className="text-[11px] font-medium text-[var(--color-text-tertiary)] uppercase tracking-wider">
                    Untracked
                  </span>
                </div>
                {untrackedTree.map((node) => (
                  <GitChangeNode
                    key={node.path}
                    node={node}
                    depth={0}
                    isUntracked={true}
                    onToggleStage={handleToggleStage}
                    onContextMenu={handleNodeContextMenu}
                    onFileClick={handleFileClick}
                  />
                ))}
              </>
            )}
          </div>
        )}
      </ScrollArea>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          placement={contextMenu.placement}
          onClose={() => setContextMenu(null)}
        />
      )}

      {workspacePath && (
        <StashDialog
          open={showStashDialog}
          onClose={() => setShowStashDialog(false)}
          workspacePath={workspacePath}
          onApply={refresh}
        />
      )}

      {/* Bottom section */}
      <div className="border-t border-[var(--color-border-primary)] bg-[var(--color-bg-page)]">
        {/* Branch bar */}
        <div
          ref={branchBarRef}
          className="flex items-center justify-between gap-2 px-3 py-2 border-b border-[var(--color-border-primary)]"
        >
          <button
            ref={branchButtonRef}
            className="flex items-center gap-1.5 min-w-0 hover:bg-[var(--color-bg-elevated)] transition-colors cursor-pointer px-1.5 py-1 -mx-1.5 rounded-none group"
            onClick={() => setShowBranchPicker((v) => !v)}
          >
            <GitBranch size={14} className="text-[var(--color-status-green)] shrink-0" />
            <span className="text-xs text-[var(--color-text-primary)] truncate font-medium group-hover:text-[var(--color-accent-blue)] transition-colors">
              {status?.remoteBranch
                ? status.remoteBranch.replace(/\//, " / ")
                : (status?.branch ?? "\u2014")}
            </span>
            <CaretDown size={12} className="text-[var(--color-text-tertiary)] shrink-0" />
          </button>

          <div className="relative flex items-center">
            <div className="flex bg-[var(--color-bg-elevated)] border border-[var(--color-border-secondary)] rounded-md overflow-hidden">
              <button
                className="flex items-center gap-1.5 px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-surface)] transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed rounded-none border-r border-[var(--color-border-secondary)]"
                onClick={() => handleRunAction()}
                disabled={actionRunning || !status?.hasRemote}
              >
                {actionDone ? (
                  <Check size={12} className="text-[var(--color-status-green)]" />
                ) : actionRunning ? (
                  <CircleNotch size={12} className="animate-spin" />
                ) : (
                  <ArrowsClockwise size={12} />
                )}
                <span className="font-medium">{currentAction.label}</span>
                {!actionRunning && !actionDone && (status?.ahead ?? 0) > 0 && (
                  <span className="flex items-center gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
                    <ArrowUp size={10} />
                    {status!.ahead}
                  </span>
                )}
                {!actionRunning && !actionDone && (status?.behind ?? 0) > 0 && (
                  <span className="flex items-center gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
                    <ArrowDown size={10} />
                    {status!.behind}
                  </span>
                )}
              </button>
              <button
                className="flex items-center px-1 py-0.5 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-surface)] transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed rounded-none"
                onClick={(e) => {
                  const rect = (e.currentTarget as HTMLButtonElement).getBoundingClientRect();
                  setContextMenu({
                    x: rect.right,
                    y: rect.top - 4,
                    placement: "bottom-end",
                    items: GIT_ACTIONS.map((action) => ({
                      label: action.label,
                      active: action.key === activeAction,
                      onClick: () => handleRunAction(action.key),
                    })),
                  });
                }}
                disabled={!status?.hasRemote}
              >
                <CaretDown size={12} />
              </button>
            </div>
          </div>
        </div>
        {showBranchPicker && activeWorkspace && (
          <BranchPicker
            workspacePath={activeWorkspace.path}
            onClose={() => setShowBranchPicker(false)}
            onSwitch={refresh}
            anchorRef={branchBarRef}
            ignoreRef={branchButtonRef}
          />
        )}

        {/* Commit input */}
        <div className="relative border-b border-[var(--color-border-primary)] bg-gradient-to-b from-[var(--color-bg-surface)] to-transparent">
          <ScrollArea className="w-full h-[140px]">
            <textarea
              className="w-full bg-transparent border-none px-4 py-4 text-[13px] text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)] resize-none focus:outline-none focus:ring-0 rounded-none overflow-hidden block"
              style={{ minHeight: "140px", paddingBottom: "3rem" }}
              placeholder="Commit message (Cmd+Enter to commit)"
              value={commitMessage}
              onChange={(e) => {
                setCommitMessage(e.target.value);
                e.target.style.height = "auto";
                e.target.style.height = e.target.scrollHeight + "px";
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                  handleCommit();
                }
              }}
            />
          </ScrollArea>
          <div className="absolute bottom-3 right-3 pointer-events-none">
            <button
              className="flex items-center gap-1.5 px-2.5 py-0.5 bg-[var(--color-bg-elevated)] border border-[var(--color-border-secondary)] text-[11px] font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-bg-surface)] hover:text-white transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed rounded-md shadow-sm pointer-events-auto"
              onClick={handleCommit}
              disabled={committing || !commitMessage.trim() || stagedCount === 0}
            >
              Commit Tracked
            </button>
          </div>
        </div>

        {/* Last commit */}
        {status?.lastCommitMessage && (
          <div className="flex items-center gap-2 px-3 py-2 bg-transparent">
            <span className="text-[11px] text-[var(--color-text-tertiary)] truncate flex-1 font-mono">
              {status.lastCommitMessage}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
