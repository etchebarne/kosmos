import type { FileTree as FileTreeModel, GitStatus, GitStatusEntry } from "@pierre/trees";
import { FileTree as PierreFileTree, useFileTree } from "@pierre/trees/react";
import {
  Check,
  Download,
  GitBranch as GitBranchIcon,
  LoaderCircle,
  Minus,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  Upload,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";

import {
  commitGitChanges,
  discardAllGitChanges,
  discardStagedGitChanges,
  fetchGitChanges,
  getGitStatus,
  pullGitChanges,
  pushGitChanges,
  stageAllGitChanges,
  stageGitPaths,
  stashGitChanges,
  switchGitBranch,
  unstageAllGitChanges,
  unstageGitPaths,
} from "@/renderer/ipc";
import { Button } from "@/renderer/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/renderer/components/ui/dropdown-menu";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/renderer/components/ui/select";
import { Separator } from "@/renderer/components/ui/separator";
import { Textarea } from "@/renderer/components/ui/textarea";
import { errorMessage } from "@/renderer/lib/errors";
import type {
  GitChange,
  GitChangeKind,
  GitRepositorySnapshot,
  GitTabParams,
  TabId,
  WorkspaceId,
} from "@/shared/ipc";

type GitTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  onActivatePane(): void;
};

type GitLoadState =
  | { status: "loading"; workspaceId: WorkspaceId; tabId: TabId }
  | {
      status: "loaded";
      workspaceId: WorkspaceId;
      tabId: TabId;
      snapshot: GitRepositorySnapshot;
      revision: number;
    }
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string };

type RemoteGitActionId = "fetch" | "pull" | "pullRebase" | "push" | "pushForce";
type GitOperationId =
  | "refresh"
  | "stageAll"
  | "unstageAll"
  | "stagePaths"
  | "unstagePaths"
  | "switchBranch"
  | "commit"
  | "stash"
  | "discardStaged"
  | "discardAll"
  | `remote:${RemoteGitActionId}`;

type RemoteGitAction = {
  id: RemoteGitActionId;
  label: string;
  icon: LucideIcon;
  confirmMessage?: string;
  run(params: GitTabParams): Promise<boolean>;
};

const CHECKBOX_HIT_WIDTH = 26;
const SUCCESS_FEEDBACK_MS = 1000;
const CHECKBOX_UNCHECKED_IMAGE = cssSvgDataUrl(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><rect x="1" y="1" width="10" height="10" rx="2" fill="none" stroke="#737373" stroke-width="1"/></svg>`,
);
const CHECKBOX_CHECKED_IMAGE = cssSvgDataUrl(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><rect x="1" y="1" width="10" height="10" rx="2" fill="#0ea5e9" stroke="#0ea5e9" stroke-width="1"/><path d="M3.1 6.1 5 8l3.9-4.2" fill="none" stroke="#ffffff" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
);
const CHECKBOX_MIXED_IMAGE = cssSvgDataUrl(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><rect x="1" y="1" width="10" height="10" rx="2" fill="#075985" stroke="#0ea5e9" stroke-width="1"/><path d="M3.2 6h5.6" fill="none" stroke="#ffffff" stroke-width="1.4" stroke-linecap="round"/></svg>`,
);
const REMOTE_GIT_ACTIONS: RemoteGitAction[] = [
  {
    id: "fetch",
    label: "Fetch",
    icon: RefreshCw,
    run: fetchGitChanges,
  },
  {
    id: "pull",
    label: "Pull",
    icon: Download,
    run: (params) => pullGitChanges({ ...params, rebase: false }),
  },
  {
    id: "pullRebase",
    label: "Pull (rebase)",
    icon: Download,
    run: (params) => pullGitChanges({ ...params, rebase: true }),
  },
  {
    id: "push",
    label: "Push",
    icon: Upload,
    run: (params) => pushGitChanges({ ...params, force: false }),
  },
  {
    id: "pushForce",
    label: "Push (force)",
    icon: Upload,
    confirmMessage: "Force push with lease?",
    run: (params) => pushGitChanges({ ...params, force: true }),
  },
];

export function GitTab({ workspaceId, tabId, onActivatePane }: GitTabProps) {
  const [loadState, setLoadState] = useState<GitLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });
  const [primaryRemoteActionId, setPrimaryRemoteActionId] = useState<RemoteGitActionId>("pull");
  const [activeOperation, setActiveOperation] = useState<GitOperationId | null>(null);
  const [successfulOperation, setSuccessfulOperation] = useState<GitOperationId | null>(null);
  const requestIdRef = useRef(0);
  const revisionRef = useRef(0);
  const successTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearSuccessTimeout = () => {
    if (successTimeoutRef.current !== null) {
      clearTimeout(successTimeoutRef.current);
      successTimeoutRef.current = null;
    }
  };
  const startOperation = (operationId: GitOperationId) => {
    clearSuccessTimeout();
    setSuccessfulOperation(null);
    setActiveOperation(operationId);
  };
  const finishOperation = () => {
    setActiveOperation(null);
  };
  const showOperationSuccess = (operationId: GitOperationId) => {
    clearSuccessTimeout();
    setSuccessfulOperation(operationId);
    successTimeoutRef.current = setTimeout(() => {
      successTimeoutRef.current = null;
      setSuccessfulOperation((currentOperation) =>
        currentOperation === operationId ? null : currentOperation,
      );
    }, SUCCESS_FEEDBACK_MS);
  };

  const loadGitStatus = async (
    targetWorkspaceId: WorkspaceId,
    targetTabId: TabId,
    showLoading: boolean,
  ) => {
    const requestId = requestIdRef.current + 1;

    requestIdRef.current = requestId;

    if (showLoading) {
      setLoadState({ status: "loading", workspaceId: targetWorkspaceId, tabId: targetTabId });
    }

    try {
      const snapshot = await getGitStatus({ workspaceId: targetWorkspaceId, tabId: targetTabId });

      if (requestIdRef.current === requestId) {
        revisionRef.current += 1;
        setLoadState({
          status: "loaded",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          snapshot,
          revision: revisionRef.current,
        });
      }
    } catch (caughtError: unknown) {
      if (requestIdRef.current === requestId) {
        setLoadState({
          status: "error",
          workspaceId: targetWorkspaceId,
          tabId: targetTabId,
          message: errorMessage(caughtError),
        });
      }
    }
  };

  useEffect(() => {
    void loadGitStatus(workspaceId, tabId, true);
  }, [workspaceId, tabId]);

  useEffect(() => {
    return () => clearSuccessTimeout();
  }, []);

  const currentLoadState: GitLoadState =
    loadState.workspaceId === workspaceId && loadState.tabId === tabId
      ? loadState
      : { status: "loading", workspaceId, tabId };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-card" onPointerDown={onActivatePane}>
      {currentLoadState.status === "loading" ? <GitMessage message="Loading repository..." /> : null}
      {currentLoadState.status === "error" ? <GitMessage message={currentLoadState.message} /> : null}
      {currentLoadState.status === "loaded" ? (
        <LoadedGitTab
          key={currentLoadState.revision}
          workspaceId={workspaceId}
          tabId={tabId}
          snapshot={currentLoadState.snapshot}
          activeOperation={activeOperation}
          successfulOperation={successfulOperation}
          primaryRemoteActionId={primaryRemoteActionId}
          onPrimaryRemoteActionChange={setPrimaryRemoteActionId}
          onOperationFinish={finishOperation}
          onOperationStart={startOperation}
          onOperationSuccess={showOperationSuccess}
          onReload={() => loadGitStatus(workspaceId, tabId, false)}
        />
      ) : null}
    </div>
  );
}

function LoadedGitTab({
  workspaceId,
  tabId,
  snapshot,
  activeOperation,
  successfulOperation,
  primaryRemoteActionId,
  onPrimaryRemoteActionChange,
  onOperationFinish,
  onOperationStart,
  onOperationSuccess,
  onReload,
}: {
  workspaceId: WorkspaceId;
  tabId: TabId;
  snapshot: GitRepositorySnapshot;
  activeOperation: GitOperationId | null;
  successfulOperation: GitOperationId | null;
  primaryRemoteActionId: RemoteGitActionId;
  onPrimaryRemoteActionChange(actionId: RemoteGitActionId): void;
  onOperationFinish(): void;
  onOperationStart(operationId: GitOperationId): void;
  onOperationSuccess(operationId: GitOperationId): void;
  onReload(): Promise<void>;
}) {
  const [commitMessage, setCommitMessage] = useState("");
  const treePaths = gitTreePaths(snapshot.changes);
  const stagedCount = snapshot.changes.filter((change) => change.isStaged).length;
  const unstagedCount = snapshot.changes.filter((change) => change.isUnstaged).length;
  const busy = activeOperation !== null;
  const hasChanges = snapshot.changes.length > 0;
  const hasCommitMessage = commitMessage.trim().length > 0;
  const runOperation = async (
    operationId: GitOperationId,
    task: () => Promise<unknown>,
    options: { refresh?: boolean; clearCommitMessage?: boolean } = {},
  ): Promise<boolean> => {
    onOperationStart(operationId);

    try {
      await task();

      if (options.clearCommitMessage) {
        setCommitMessage("");
      }

      if (options.refresh !== false) {
        await onReload();
      }

      onOperationSuccess(operationId);

      return true;
    } catch (caughtError: unknown) {
      window.alert(errorMessage(caughtError));
      return false;
    } finally {
      onOperationFinish();
    }
  };
  const tabParams = { workspaceId, tabId };
  const toggleStagePaths = (paths: string[]) => {
    const shouldUnstage = paths.every((path) => changeForPath(snapshot.changes, path)?.isStaged);

    void runOperation(shouldUnstage ? "unstagePaths" : "stagePaths", () =>
      shouldUnstage
        ? unstageGitPaths({ ...tabParams, paths })
        : stageGitPaths({ ...tabParams, paths }),
    );
  };
  const switchBranchValue = (branch: string | null) => {
    if (!branch || branch === snapshot.branch) {
      return;
    }

    void runOperation("switchBranch", () => switchGitBranch({ ...tabParams, branch }));
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex min-h-10 shrink-0 items-center gap-2 border-b px-2 py-1.5">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          disabled={busy}
          aria-label="Refresh git status"
          onClick={() => void runOperation("refresh", onReload, { refresh: false })}
        >
          <OperationIcon
            defaultIcon={RefreshCw}
            operationId="refresh"
            activeOperation={activeOperation}
            successfulOperation={successfulOperation}
          />
        </Button>
        <div className="min-w-0 flex-1">
          <GitChangeSummary snapshot={snapshot} />
        </div>
        <div className="flex shrink-0 items-center gap-0">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            disabled={busy || unstagedCount === 0}
            aria-label="Stage all changes"
            onClick={() => void runOperation("stageAll", () => stageAllGitChanges(tabParams))}
          >
            <OperationIcon
              defaultIcon={Plus}
              operationId="stageAll"
              activeOperation={activeOperation}
              successfulOperation={successfulOperation}
            />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            disabled={busy || stagedCount === 0}
            aria-label="Unstage all changes"
            onClick={() => void runOperation("unstageAll", () => unstageAllGitChanges(tabParams))}
          >
            <OperationIcon
              defaultIcon={Minus}
              operationId="unstageAll"
              activeOperation={activeOperation}
              successfulOperation={successfulOperation}
            />
          </Button>
          <GitActionsMenu
            activeOperation={activeOperation}
            successfulOperation={successfulOperation}
            busy={busy}
            hasChanges={hasChanges}
            hasStagedChanges={stagedCount > 0}
            tabParams={tabParams}
            onRun={runOperation}
          />
        </div>
      </div>

      <div className="relative min-h-0 flex-1 overflow-hidden border-b">
        {hasChanges ? (
          <GitChangeTree
            key={snapshot.changes.map((change) => `${change.path}:${change.staged}:${change.unstaged}`).join("|")}
            changes={snapshot.changes}
            paths={treePaths}
            disabled={busy}
            onToggleStage={toggleStagePaths}
          />
        ) : (
          <GitMessage message="No changes" />
        )}
      </div>

      <div className="flex shrink-0 flex-col gap-2 p-2">
        <div className="flex items-center justify-between gap-2">
          <Select value={snapshot.branch ?? null} disabled={busy || snapshot.branches.length === 0} onValueChange={switchBranchValue}>
            <SelectTrigger size="sm" className="min-w-44 max-w-72 flex-1 justify-start">
              {activeOperation === "switchBranch" ? (
                <LoaderCircle className="size-3.5 animate-spin text-muted-foreground" />
              ) : successfulOperation === "switchBranch" ? (
                <Check className="size-3.5 text-emerald-500" />
              ) : (
                <GitBranchIcon className="size-3.5 text-muted-foreground" />
              )}
              <SelectValue placeholder="Detached HEAD" />
            </SelectTrigger>
            <SelectContent align="start" className="max-w-80">
              {snapshot.branches.map((branch) => (
                <SelectItem key={branch.name} value={branch.name}>
                  <span className="min-w-0 truncate">{branch.name}</span>
                  {branch.upstream ? (
                    <span className="text-xs text-muted-foreground">{branch.upstream}</span>
                  ) : null}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <RemoteGitActions
            busy={busy}
            activeOperation={activeOperation}
            successfulOperation={successfulOperation}
            primaryActionId={primaryRemoteActionId}
            tabParams={tabParams}
            onPrimaryActionChange={onPrimaryRemoteActionChange}
            onRun={runOperation}
          />
        </div>
        <Textarea
          value={commitMessage}
          placeholder="Commit message"
          className="max-h-28 min-h-20 resize-none border-0 bg-background/60 text-sm shadow-none focus-visible:ring-1"
          disabled={busy}
          onChange={(event) => setCommitMessage(event.target.value)}
        />
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1" />
          <Button
            type="button"
            disabled={busy || stagedCount === 0 || !hasCommitMessage}
            onClick={() =>
              void runOperation(
                "commit",
                () => commitGitChanges({ ...tabParams, message: commitMessage }),
                { clearCommitMessage: true },
              )
            }
          >
            {activeOperation === "commit" ? <LoaderCircle className="animate-spin" /> : null}
            {activeOperation !== "commit" && successfulOperation === "commit" ? (
              <Check className="text-emerald-500" />
            ) : null}
            Commit staged
          </Button>
        </div>
        <GitLatestCommitFooter latestCommit={snapshot.latestCommit} />
      </div>
    </div>
  );
}

function GitLatestCommitFooter({ latestCommit }: { latestCommit?: string | null }) {
  return (
    <div className="-mx-2 -mb-2 border-t px-2 py-1.5 text-xs text-muted-foreground">
      <p className="truncate">{latestCommit ?? "No commits yet"}</p>
    </div>
  );
}

function RemoteGitActions({
  activeOperation,
  successfulOperation,
  busy,
  primaryActionId,
  tabParams,
  onPrimaryActionChange,
  onRun,
}: {
  activeOperation: GitOperationId | null;
  successfulOperation: GitOperationId | null;
  busy: boolean;
  primaryActionId: RemoteGitActionId;
  tabParams: GitTabParams;
  onPrimaryActionChange(actionId: RemoteGitActionId): void;
  onRun(
    operationId: GitOperationId,
    operation: () => Promise<unknown>,
    options?: { refresh?: boolean; clearCommitMessage?: boolean },
  ): Promise<boolean>;
}) {
  const primaryAction = remoteGitAction(primaryActionId);
  const primaryOperationId = remoteOperationId(primaryAction.id);
  const runRemoteAction = (action: RemoteGitAction) => {
    if (action.confirmMessage && !window.confirm(action.confirmMessage)) {
      return;
    }

    const previousActionId = primaryActionId;
    onPrimaryActionChange(action.id);

    void onRun(remoteOperationId(action.id), () => action.run(tabParams)).then((succeeded) => {
      if (succeeded) {
        onPrimaryActionChange(action.id);
      } else {
        onPrimaryActionChange(previousActionId);
      }
    });
  };

  return (
    <div className="flex shrink-0 items-center gap-1">
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={busy}
        onClick={() => runRemoteAction(primaryAction)}
      >
        <OperationIcon
          defaultIcon={primaryAction.icon}
          operationId={primaryOperationId}
          activeOperation={activeOperation}
          successfulOperation={successfulOperation}
        />
        {primaryAction.label}
      </Button>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button type="button" variant="ghost" size="icon-sm" disabled={busy} aria-label="Remote git actions">
              <MoreHorizontal />
            </Button>
          }
        />
        <DropdownMenuContent align="end" className="w-44">
          <DropdownMenuGroup>
            {REMOTE_GIT_ACTIONS.filter((action) => action.id !== primaryActionId).map((action) => {
              const ActionIcon = action.icon;

              return (
                <DropdownMenuItem key={action.id} onClick={() => runRemoteAction(action)}>
                  <ActionIcon />
                  {action.label}
                </DropdownMenuItem>
              );
            })}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function GitActionsMenu({
  activeOperation,
  successfulOperation,
  busy,
  hasChanges,
  hasStagedChanges,
  tabParams,
  onRun,
}: {
  activeOperation: GitOperationId | null;
  successfulOperation: GitOperationId | null;
  busy: boolean;
  hasChanges: boolean;
  hasStagedChanges: boolean;
  tabParams: GitTabParams;
  onRun(
    operationId: GitOperationId,
    operation: () => Promise<unknown>,
    options?: { refresh?: boolean; clearCommitMessage?: boolean },
  ): Promise<boolean>;
}) {
  const localActionRunning = isLocalMenuOperation(activeOperation);
  const localActionSucceeded = isLocalMenuOperation(successfulOperation);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button type="button" variant="ghost" size="icon-sm" disabled={busy} aria-label="Git actions">
            {localActionRunning ? <LoaderCircle className="animate-spin" /> : null}
            {!localActionRunning && localActionSucceeded ? <Check className="text-emerald-500" /> : null}
            {!localActionRunning && !localActionSucceeded ? <MoreHorizontal /> : null}
          </Button>
        }
      />
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Local</DropdownMenuLabel>
          <DropdownMenuItem disabled={!hasChanges} onClick={() => void onRun("stash", () => stashGitChanges(tabParams))}>
            <Save />
            Stash
          </DropdownMenuItem>
          <DropdownMenuItem
            variant="destructive"
            disabled={!hasStagedChanges}
            onClick={() => {
              if (!window.confirm("Discard staged changes? This cannot be undone.")) {
                return;
              }

              void onRun("discardStaged", () => discardStagedGitChanges(tabParams));
            }}
          >
            <Trash2 />
            Discard staged
          </DropdownMenuItem>
          <DropdownMenuItem
            variant="destructive"
            disabled={!hasChanges}
            onClick={() => {
              if (!window.confirm("Discard all changes? This cannot be undone.")) {
                return;
              }

              void onRun("discardAll", () => discardAllGitChanges(tabParams));
            }}
          >
            <Trash2 />
            Discard all
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function remoteGitAction(actionId: RemoteGitActionId): RemoteGitAction {
  return REMOTE_GIT_ACTIONS.find((action) => action.id === actionId) ?? REMOTE_GIT_ACTIONS[0]!;
}

function remoteOperationId(actionId: RemoteGitActionId): GitOperationId {
  return `remote:${actionId}`;
}

function isLocalMenuOperation(operation: GitOperationId | null): boolean {
  return operation === "stash" || operation === "discardStaged" || operation === "discardAll";
}

function GitChangeSummary({ snapshot }: { snapshot: GitRepositorySnapshot }) {
  return (
    <p className="truncate text-xs font-medium">
      <span>{changeCountLabel(snapshot.changes.length)}</span>
      <span className="ml-2 text-emerald-500">+{snapshot.insertions}</span>
      <span className="ml-1.5 text-red-500">-{snapshot.deletions}</span>
    </p>
  );
}

function OperationIcon({
  activeOperation,
  defaultIcon: DefaultIcon,
  operationId,
  successfulOperation,
}: {
  activeOperation: GitOperationId | null;
  defaultIcon: LucideIcon;
  operationId: GitOperationId;
  successfulOperation: GitOperationId | null;
}) {
  if (activeOperation === operationId) {
    return <LoaderCircle className="animate-spin" />;
  }

  if (successfulOperation === operationId) {
    return <Check className="text-emerald-500" />;
  }

  return <DefaultIcon />;
}

function GitChangeTree({
  changes,
  paths,
  disabled,
  onToggleStage,
}: {
  changes: GitChange[];
  paths: string[];
  disabled: boolean;
  onToggleStage(paths: string[]): void;
}) {
  const { model } = useFileTree({
    density: "compact",
    flattenEmptyDirectories: false,
    gitStatus: gitStatusEntries(changes),
    initialExpansion: "open",
    paths,
    renderRowDecoration: ({ item }) => ({ text: statusLabel(changes, item.path), title: statusTitle(changes, item.path) }),
    stickyFolders: true,
    unsafeCSS: gitTreeCheckboxCss(changes, paths, disabled),
  });

  return (
    <>
      <PierreFileTree
        model={model}
        className="h-full min-h-0 w-full overflow-hidden bg-card text-card-foreground [--trees-accent-override:var(--accent)] [--trees-bg-muted-override:var(--accent)] [--trees-bg-override:var(--card)] [--trees-border-color-override:var(--border)] [--trees-fg-muted-override:var(--muted-foreground)] [--trees-fg-override:var(--card-foreground)] [--trees-focus-ring-color-override:var(--ring)] [--trees-input-bg-override:var(--input)] [--trees-item-row-gap-override:6px] [--trees-padding-inline-override:0px] [--trees-scrollbar-gutter-override:0px] [--trees-search-bg-override:var(--input)] [--trees-search-fg-override:var(--foreground)] [--trees-selected-bg-override:var(--accent)] [--trees-selected-fg-override:var(--accent-foreground)] [--trees-selected-focused-border-color-override:var(--ring)]"
        style={{ height: "100%" }}
      />
      <GitStageCheckboxController
        model={model}
        changes={changes}
        disabled={disabled}
        onToggleStage={onToggleStage}
      />
    </>
  );
}

function GitStageCheckboxController({
  model,
  changes,
  disabled,
  onToggleStage,
}: {
  model: FileTreeModel;
  changes: GitChange[];
  disabled: boolean;
  onToggleStage(paths: string[]): void;
}) {
  useEffect(() => {
    if (disabled) {
      return undefined;
    }

    let frameId = 0;
    let cleanup: (() => void) | undefined;
    const attach = () => {
      const shadowRoot = model.getFileTreeContainer()?.shadowRoot;

      if (!shadowRoot) {
        frameId = requestAnimationFrame(attach);
        return;
      }

      const handlePointerDown = (event: Event) => {
        if (!(event instanceof PointerEvent)) {
          return;
        }

        const row = rowElementFromEvent(event);
        if (!row) {
          return;
        }

        const rowRect = row.getBoundingClientRect();
        if (event.clientX > rowRect.left + CHECKBOX_HIT_WIDTH) {
          return;
        }

        const path = row.dataset.itemPath;
        if (!path) {
          return;
        }

        const paths = changedPathsForTreePath(changes, path);
        if (paths.length === 0) {
          return;
        }

        event.preventDefault();
        event.stopPropagation();
        onToggleStage(paths);
      };

      shadowRoot.addEventListener("pointerdown", handlePointerDown, true);
      cleanup = () => shadowRoot.removeEventListener("pointerdown", handlePointerDown, true);
    };

    frameId = requestAnimationFrame(attach);

    return () => {
      cancelAnimationFrame(frameId);
      cleanup?.();
    };
  }, [model, changes, disabled, onToggleStage]);

  return null;
}

function rowElementFromEvent(event: PointerEvent): HTMLElement | null {
  for (const element of event.composedPath()) {
    if (!(element instanceof HTMLElement)) {
      continue;
    }

    if (element.dataset.type === "item" && element.dataset.itemPath !== undefined) {
      return element;
    }
  }

  return null;
}

function gitTreePaths(changes: GitChange[]): string[] {
  const paths = new Set<string>();

  for (const change of changes) {
    for (const parentPath of parentDirectoryPaths(change.path)) {
      paths.add(parentPath);
    }

    paths.add(change.path);
  }

  return [...paths].sort((left, right) => left.localeCompare(right));
}

function parentDirectoryPaths(path: string): string[] {
  const segments = path.split("/").filter(Boolean);
  const paths: string[] = [];

  for (let index = 1; index < segments.length; index += 1) {
    paths.push(`${segments.slice(0, index).join("/")}/`);
  }

  return paths;
}

function gitStatusEntries(changes: GitChange[]): GitStatusEntry[] {
  return changes.map((change) => ({ path: change.path, status: gitStatus(change) }));
}

function gitStatus(change: GitChange): GitStatus {
  return gitStatusFromKind(change.unstaged ?? change.staged ?? "modified");
}

function gitStatusFromKind(kind: GitChangeKind): GitStatus {
  switch (kind) {
    case "added":
      return "added";
    case "deleted":
      return "deleted";
    case "ignored":
      return "ignored";
    case "renamed":
      return "renamed";
    case "untracked":
      return "untracked";
    case "conflicted":
    case "modified":
      return "modified";
  }
}

function gitTreeCheckboxCss(changes: GitChange[], paths: string[], disabled: boolean): string {
  const stageState = stageStateForTreePaths(changes, paths);
  const checkedSelectors = selectorList(stageState.checked);
  const mixedSelectors = selectorList(stageState.mixed);

  return [
    `[data-type="item"]{padding-left:24px!important;background-image:${CHECKBOX_UNCHECKED_IMAGE};background-repeat:no-repeat;background-size:12px 12px;background-position:6px center;${disabled ? "opacity:.7;" : ""}}`,
    checkedSelectors.length > 0
      ? `${checkedSelectors}{background-image:${CHECKBOX_CHECKED_IMAGE};}`
      : "",
    mixedSelectors.length > 0
      ? `${mixedSelectors}{background-image:${CHECKBOX_MIXED_IMAGE};}`
      : "",
  ].join("\n");
}

function cssSvgDataUrl(svg: string): string {
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

function stageStateForTreePaths(
  changes: GitChange[],
  paths: string[],
): { checked: string[]; mixed: string[] } {
  const checked: string[] = [];
  const mixed: string[] = [];

  for (const path of paths) {
    const pathChanges = changedPathsForTreePath(changes, path)
      .map((changedPath) => changeForPath(changes, changedPath))
      .filter((change): change is GitChange => Boolean(change));

    if (pathChanges.length === 0) {
      continue;
    }

    const stagedCount = pathChanges.filter((change) => change.isStaged).length;

    if (stagedCount === 0) {
      continue;
    }

    if (stagedCount === pathChanges.length) {
      checked.push(path);
    } else {
      mixed.push(path);
    }
  }

  return { checked, mixed };
}

function selectorList(paths: string[]): string {
  return paths.map((path) => `[data-type="item"][data-item-path=${cssString(path)}]`).join(",");
}

function cssString(value: string): string {
  return JSON.stringify(value);
}

function changedPathsForTreePath(changes: GitChange[], path: string): string[] {
  if (path.endsWith("/")) {
    return changes.filter((change) => change.path.startsWith(path)).map((change) => change.path);
  }

  return changes.some((change) => change.path === path) ? [path] : [];
}

function changeForPath(changes: GitChange[], path: string): GitChange | undefined {
  return changes.find((change) => change.path === path);
}

function statusLabel(changes: GitChange[], path: string): string {
  const change = changeForPath(changes, path);

  if (!change) {
    return "";
  }

  return `${statusCode(change.staged)}${statusCode(change.unstaged)}`;
}

function statusTitle(changes: GitChange[], path: string): string {
  const change = changeForPath(changes, path);

  if (!change) {
    return "Directory contains changes";
  }

  const parts = [
    change.staged ? `Staged: ${change.staged}` : null,
    change.unstaged ? `Unstaged: ${change.unstaged}` : null,
    change.originalPath ? `Originally: ${change.originalPath}` : null,
  ].filter(Boolean);

  return parts.join("\n");
}

function statusCode(kind: GitChangeKind | null | undefined): string {
  switch (kind) {
    case "added":
      return "A";
    case "conflicted":
      return "U";
    case "deleted":
      return "D";
    case "ignored":
      return "I";
    case "modified":
      return "M";
    case "renamed":
      return "R";
    case "untracked":
      return "?";
    case null:
    case undefined:
      return " ";
  }
}

function changeCountLabel(changeCount: number): string {
  return changeCount === 1 ? "1 Change" : `${changeCount} Changes`;
}

function GitMessage({ message }: { message: string }) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <div className="flex max-w-sm flex-col items-center gap-3">
        <Separator className="w-12" />
        <p className="text-sm text-muted-foreground">{message}</p>
      </div>
    </div>
  );
}
