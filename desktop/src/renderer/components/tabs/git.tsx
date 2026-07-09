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
  Undo2,
  type LucideIcon,
} from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

import {
  applyGitStash,
  commitGitChanges,
  createGitBranch,
  deleteGitBranch,
  discardAllGitChanges,
  discardStagedGitChanges,
  dropGitStash,
  fetchGitChanges,
  getGitStashes,
  getGitStatus,
  initGitRepository,
  pullGitChanges,
  pushGitChanges,
  stageAllGitChanges,
  stageGitPaths,
  stashStagedGitChanges,
  switchGitBranch,
  trackGitRemoteBranch,
  unstageAllGitChanges,
  unstageGitPaths,
} from "@/renderer/ipc";
import { Button } from "@/renderer/components/ui/button";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/renderer/components/ui/card";
import { Checkbox } from "@/renderer/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/renderer/components/ui/dialog";
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
import { Textarea } from "@/renderer/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/renderer/components/ui/tooltip";
import { errorMessage } from "@/renderer/lib/errors";
import type {
  GitChange,
  GitChangeKind,
  GitBranch,
  GitRepositorySnapshot,
  GitStash,
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
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string; code?: string };

type RemoteGitActionId = "fetch" | "pull" | "pullRebase" | "push" | "pushForce";
type GitOperationId =
  | "refresh"
  | "stageAll"
  | "unstageAll"
  | "stagePaths"
  | "unstagePaths"
  | "init"
  | "switchBranch"
  | "trackRemoteBranch"
  | "createBranch"
  | "deleteBranch"
  | "commit"
  | "stashStaged"
  | "applyStash"
  | "dropStash"
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

type GitStashListState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "loaded"; stashes: GitStash[] }
  | { status: "error"; message: string };

type ActiveStashAction = {
  selector: string;
  operationId: "applyStash" | "dropStash";
};

type StageCheckboxState = "checked" | "mixed" | "unchecked";

type StageCheckboxOverlayItem = {
  centerY: number;
  path: string;
  paths: string[];
  state: StageCheckboxState;
  title: string;
};

const OPERATION_FEEDBACK_DISABLED = new Set<GitOperationId>(["stageAll", "unstageAll"]);
const GIT_REPOSITORY_NOT_FOUND_CODE = "git.repository_not_found";

const TREE_CHECKBOX_LEFT = 5;
const SUCCESS_FEEDBACK_MS = 1000;
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
  const [stashDialogOpen, setStashDialogOpen] = useState(false);
  const [branchDialogOpen, setBranchDialogOpen] = useState(false);
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
    if (OPERATION_FEEDBACK_DISABLED.has(operationId)) {
      return;
    }

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
          code: ipcErrorCode(caughtError),
        });
      }
    }
  };

  const initializeGitRepository = async () => {
    startOperation("init");

    try {
      await initGitRepository({ workspaceId, tabId });
      await loadGitStatus(workspaceId, tabId, false);
    } catch (caughtError: unknown) {
      window.alert(errorMessage(caughtError));
    } finally {
      finishOperation();
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
      {currentLoadState.status === "error" && currentLoadState.code === GIT_REPOSITORY_NOT_FOUND_CODE ? (
        <GitInitializeRepositoryMessage
          busy={activeOperation === "init"}
          onInitialize={() => void initializeGitRepository()}
        />
      ) : null}
      {currentLoadState.status === "error" && currentLoadState.code !== GIT_REPOSITORY_NOT_FOUND_CODE ? (
        <GitMessage message={currentLoadState.message} />
      ) : null}
      {currentLoadState.status === "loaded" ? (
        <LoadedGitTab
          key={currentLoadState.revision}
          workspaceId={workspaceId}
          tabId={tabId}
          snapshot={currentLoadState.snapshot}
          activeOperation={activeOperation}
          successfulOperation={successfulOperation}
          primaryRemoteActionId={primaryRemoteActionId}
          stashDialogOpen={stashDialogOpen}
          branchDialogOpen={branchDialogOpen}
          onPrimaryRemoteActionChange={setPrimaryRemoteActionId}
          onStashDialogOpenChange={setStashDialogOpen}
          onBranchDialogOpenChange={setBranchDialogOpen}
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
  stashDialogOpen,
  branchDialogOpen,
  onPrimaryRemoteActionChange,
  onStashDialogOpenChange,
  onBranchDialogOpenChange,
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
  stashDialogOpen: boolean;
  branchDialogOpen: boolean;
  onPrimaryRemoteActionChange(actionId: RemoteGitActionId): void;
  onStashDialogOpenChange(open: boolean): void;
  onBranchDialogOpenChange(open: boolean): void;
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
  const hasCommittedHistory = snapshot.latestCommit != null;
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
  const switchBranch = (branch: string) => {
    if (branch === snapshot.branch) {
      return;
    }

    onBranchDialogOpenChange(false);
    void runOperation("switchBranch", () => switchGitBranch({ ...tabParams, branch }));
  };
  const trackRemoteBranch = (branch: string) => {
    const localBranch = localBranchNameFromRemote(branch);

    if (snapshot.branches.some((candidate) => !candidate.remote && candidate.name === localBranch)) {
      switchBranch(localBranch);
      return;
    }

    onBranchDialogOpenChange(false);
    void runOperation("trackRemoteBranch", () => trackGitRemoteBranch({ ...tabParams, branch }));
  };
  const createBranch = (name: string, startPoint: string) =>
    runOperation("createBranch", () => createGitBranch({ ...tabParams, name, startPoint }));
  const deleteBranch = (branch: string) =>
    runOperation("deleteBranch", () => deleteGitBranch({ ...tabParams, branch }));

  return (
    <TooltipProvider>
      <div className="flex h-full min-h-0 flex-col overflow-hidden">
        <div className="flex min-h-10 shrink-0 items-center gap-2 border-b px-2 py-1.5">
          <GitToolbarTooltip label="Refresh git status">
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
          </GitToolbarTooltip>
          <div className="min-w-0 flex-1">
            <GitChangeSummary snapshot={snapshot} />
          </div>
          <div className="flex shrink-0 items-center gap-0">
            <GitToolbarTooltip label="Stage all changes">
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
            </GitToolbarTooltip>
            <GitToolbarTooltip label="Unstage all changes">
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
            </GitToolbarTooltip>
            <GitToolbarTooltip label="Stash staged changes">
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                disabled={busy || stagedCount === 0}
                aria-label="Stash staged changes"
                onClick={() => void runOperation("stashStaged", () => stashStagedGitChanges(tabParams))}
              >
                <OperationIcon
                  defaultIcon={Save}
                  operationId="stashStaged"
                  activeOperation={activeOperation}
                  successfulOperation={successfulOperation}
                />
              </Button>
            </GitToolbarTooltip>
            <GitActionsMenu
              activeOperation={activeOperation}
              successfulOperation={successfulOperation}
              busy={busy}
              canDiscardAll={hasCommittedHistory && hasChanges}
              canDiscardStaged={hasCommittedHistory && stagedCount > 0}
              tabParams={tabParams}
              onOpenStashes={() => onStashDialogOpenChange(true)}
              onRun={runOperation}
            />
          </div>
        </div>

        <GitStashesDialog
          open={stashDialogOpen}
          busy={busy}
          tabParams={tabParams}
          onOpenChange={onStashDialogOpenChange}
          onRun={runOperation}
        />

        <GitBranchesDialog
          open={branchDialogOpen}
          branches={snapshot.branches}
          currentBranch={snapshot.branch}
          busy={busy}
          onOpenChange={onBranchDialogOpenChange}
          onCreate={createBranch}
          onDelete={deleteBranch}
          onSwitch={switchBranch}
          onTrackRemote={trackRemoteBranch}
        />

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
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="min-w-0 max-w-72 shrink justify-start overflow-hidden"
              disabled={busy || snapshot.branches.length === 0}
              aria-label="Select git branch"
              onClick={() => onBranchDialogOpenChange(true)}
            >
              {activeOperation === "switchBranch" || activeOperation === "trackRemoteBranch" ? (
                <LoaderCircle className="size-3.5 animate-spin text-muted-foreground" />
              ) : successfulOperation === "switchBranch" || successfulOperation === "trackRemoteBranch" ? (
                <Check className="size-3.5 text-emerald-500" />
              ) : (
                <GitBranchIcon className="size-3.5 text-muted-foreground" />
              )}
              <span className="min-w-0 truncate">{snapshot.branch ?? "Detached HEAD"}</span>
            </Button>
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
    </TooltipProvider>
  );
}

function GitLatestCommitFooter({ latestCommit }: { latestCommit?: string | null }) {
  return (
    <div className="-mx-2 -mb-2 border-t px-2 py-1.5 text-xs text-muted-foreground">
      <p className="truncate">{latestCommit ?? "No commits yet"}</p>
    </div>
  );
}

function GitToolbarTooltip({ children, label }: { children: ReactNode; label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="inline-flex" />}>{children}</TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  );
}

function GitBranchesDialog({
  branches,
  busy,
  currentBranch,
  open,
  onCreate,
  onDelete,
  onOpenChange,
  onSwitch,
  onTrackRemote,
}: {
  branches: GitBranch[];
  busy: boolean;
  currentBranch?: string | null;
  open: boolean;
  onCreate(name: string, startPoint: string): Promise<boolean>;
  onDelete(branch: string): Promise<boolean>;
  onOpenChange(open: boolean): void;
  onSwitch(branch: string): void;
  onTrackRemote(branch: string): void;
}) {
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [search, setSearch] = useState("");
  const localBranchNames = new Set(branches.filter((branch) => !branch.remote).map((branch) => branch.name));
  const localBranches = branches.filter((branch) => !branch.remote && branchMatchesSearch(branch, search));
  const remoteBranches = branches.filter((branch) => branch.remote && branchMatchesSearch(branch, search));

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="!flex max-h-[min(34rem,calc(100vh-2rem))] max-w-xl flex-col overflow-hidden p-0">
          <div className="px-4 pt-4 pr-12">
            <DialogHeader>
              <DialogTitle>Branches</DialogTitle>
              <DialogDescription>Switch local branches or create one from an existing branch.</DialogDescription>
            </DialogHeader>
          </div>

          <div className="flex gap-2 px-4">
            <input
              value={search}
              placeholder="Search branches"
              className="h-8 min-w-0 flex-1 rounded-lg border bg-background px-2 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring"
              onChange={(event) => setSearch(event.target.value)}
            />
            <Button
              type="button"
              size="sm"
              disabled={busy || branches.length === 0}
              onClick={() => setCreateDialogOpen(true)}
            >
              <Plus />
              New
            </Button>
          </div>

          <div className="scrollbar-themed min-h-0 flex-1 overflow-y-auto px-4 pb-4">
            {localBranches.length === 0 && remoteBranches.length === 0 ? (
              <GitDialogMessage message={search.trim() ? "No matching branches" : "No branches"} />
            ) : null}
            {localBranches.length > 0 ? (
              <GitBranchSection
                title="Local"
                branches={localBranches}
                busy={busy}
                currentBranch={currentBranch}
                onDelete={onDelete}
                onSwitch={onSwitch}
              />
            ) : null}
            {remoteBranches.length > 0 ? (
              <GitBranchSection
                title="Remote"
                branches={remoteBranches}
                busy={busy}
                currentBranch={currentBranch}
                localBranchNames={localBranchNames}
                onSwitch={onSwitch}
                onTrackRemote={onTrackRemote}
              />
            ) : null}
          </div>
        </DialogContent>
      </Dialog>

      <GitCreateBranchDialog
        open={createDialogOpen}
        branches={branches}
        busy={busy}
        currentBranch={currentBranch}
        onCreate={onCreate}
        onCreated={() => onOpenChange(false)}
        onOpenChange={setCreateDialogOpen}
      />
    </>
  );
}

function GitCreateBranchDialog({
  branches,
  busy,
  currentBranch,
  open,
  onCreate,
  onCreated,
  onOpenChange,
}: {
  branches: GitBranch[];
  busy: boolean;
  currentBranch?: string | null;
  open: boolean;
  onCreate(name: string, startPoint: string): Promise<boolean>;
  onCreated(): void;
  onOpenChange(open: boolean): void;
}) {
  const [name, setName] = useState("");
  const [source, setSource] = useState("");
  const mountedRef = useRef(true);
  const defaultSource = currentBranch && branches.some((branch) => branch.name === currentBranch)
    ? currentBranch
    : branches[0]?.name ?? "";
  const canCreate = name.trim().length > 0 && source.trim().length > 0;

  useEffect(() => {
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }

    setName("");
    setSource(defaultSource);
  }, [open]);

  const createBranch = async () => {
    const branchName = name.trim();
    const startPoint = source.trim();

    if (!branchName || !startPoint) {
      return;
    }

    const succeeded = await onCreate(branchName, startPoint);

    if (!succeeded) {
      return;
    }

    onCreated();

    if (mountedRef.current) {
      onOpenChange(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md overflow-hidden p-0">
        <div className="px-4 pt-4 pr-12">
          <DialogHeader>
            <DialogTitle>New branch</DialogTitle>
            <DialogDescription>Create and switch to a new branch from a selected source.</DialogDescription>
          </DialogHeader>
        </div>

        <div className="space-y-3 px-4 pb-4">
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-muted-foreground">Branch name</span>
            <input
              value={name}
              placeholder="feature/my-branch"
              className="h-8 w-full rounded-lg border bg-background px-2 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring"
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className="block space-y-1.5">
            <span className="text-xs font-medium text-muted-foreground">Create from</span>
            <Select value={source} disabled={branches.length === 0} onValueChange={(value) => setSource(value ?? "")}>
              <SelectTrigger size="sm" className="w-full justify-start">
                <SelectValue placeholder="Select source branch" />
              </SelectTrigger>
              <SelectContent align="start" className="max-w-80">
                {branches.map((branch) => (
                  <SelectItem key={branch.name} value={branch.name}>
                    <span className="min-w-0 truncate">{branch.name}</span>
                    <span className="text-xs text-muted-foreground">{branch.remote ? "Remote" : "Local"}</span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
        </div>

        <DialogFooter className="mx-0 mb-0 rounded-none bg-muted/30 px-4 py-3">
          <Button type="button" variant="outline" disabled={busy} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="button" disabled={busy || !canCreate} onClick={() => void createBranch()}>
            {busy ? <LoaderCircle className="animate-spin" /> : null}
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function GitBranchSection({
  branches,
  busy,
  currentBranch,
  localBranchNames,
  title,
  onDelete,
  onSwitch,
  onTrackRemote,
}: {
  branches: GitBranch[];
  busy: boolean;
  currentBranch?: string | null;
  localBranchNames?: ReadonlySet<string>;
  title: string;
  onDelete?(branch: string): Promise<boolean>;
  onSwitch(branch: string): void;
  onTrackRemote?(branch: string): void;
}) {
  return (
    <section className="mt-4 space-y-1.5 first:mt-0">
      <h3 className="px-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">{title}</h3>
      <ul className="space-y-1.5">
        {branches.map((branch) => (
          <GitBranchRow
            key={branch.name}
            branch={branch}
            busy={busy}
            current={branch.name === currentBranch}
            localBranchNames={localBranchNames}
            onDelete={onDelete}
            onSwitch={onSwitch}
            onTrackRemote={onTrackRemote}
          />
        ))}
      </ul>
    </section>
  );
}

function GitBranchRow({
  branch,
  busy,
  current,
  localBranchNames,
  onDelete,
  onSwitch,
  onTrackRemote,
}: {
  branch: GitBranch;
  busy: boolean;
  current: boolean;
  localBranchNames?: ReadonlySet<string>;
  onDelete?(branch: string): Promise<boolean>;
  onSwitch(branch: string): void;
  onTrackRemote?(branch: string): void;
}) {
  const canSwitch = (!branch.remote && !current) || (branch.remote && Boolean(onTrackRemote));
  const canDelete = Boolean(onDelete && !branch.remote && !current);
  const localBranchName = localBranchNameFromRemote(branch.name);
  const branchDescription = branch.remote
    ? `${localBranchNames?.has(localBranchName) ? "Switch local" : "Create local"} ${localBranchName}`
    : branch.upstream;
  const content = (
    <>
      <GitBranchIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-medium">{branch.name}</span>
        {branchDescription ? (
          <span className="block truncate text-xs text-muted-foreground">{branchDescription}</span>
        ) : null}
      </span>
    </>
  );

  return (
    <li className="flex items-center gap-1.5 rounded-lg border bg-background/60 p-1.5">
      {canSwitch ? (
        <button
          type="button"
          disabled={busy}
          className="flex min-w-0 flex-1 items-center gap-3 rounded-md px-1.5 py-1 text-left text-sm transition-colors hover:bg-muted/60 disabled:cursor-default disabled:opacity-60"
          onClick={() => (branch.remote ? onTrackRemote?.(branch.name) : onSwitch(branch.name))}
        >
          {content}
        </button>
      ) : (
        <div className="flex min-w-0 flex-1 items-center gap-3 px-1.5 py-1 text-sm">{content}</div>
      )}
      {current ? <Check className="mx-1 size-3.5 shrink-0 text-emerald-500" /> : null}
      {canDelete ? (
        <Button
          type="button"
          variant="destructive"
          size="icon-sm"
          disabled={busy}
          aria-label={`Delete ${branch.name}`}
          onClick={() => {
            if (!window.confirm(`Delete local branch ${branch.name}?`)) {
              return;
            }

            void onDelete?.(branch.name);
          }}
        >
          <Trash2 />
        </Button>
      ) : null}
    </li>
  );
}

function branchMatchesSearch(branch: GitBranch, search: string): boolean {
  const query = search.trim().toLowerCase();

  if (!query) {
    return true;
  }

  return branch.name.toLowerCase().includes(query) || Boolean(branch.upstream?.toLowerCase().includes(query));
}

function localBranchNameFromRemote(branch: string): string {
  const separatorIndex = branch.indexOf("/");

  if (separatorIndex === -1 || separatorIndex === branch.length - 1) {
    return branch;
  }

  return branch.slice(separatorIndex + 1);
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
  canDiscardStaged,
  successfulOperation,
  busy,
  canDiscardAll,
  tabParams,
  onOpenStashes,
  onRun,
}: {
  activeOperation: GitOperationId | null;
  canDiscardStaged: boolean;
  successfulOperation: GitOperationId | null;
  busy: boolean;
  canDiscardAll: boolean;
  tabParams: GitTabParams;
  onOpenStashes(): void;
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
      <GitToolbarTooltip label="More git actions">
        <DropdownMenuTrigger
          render={
            <Button type="button" variant="ghost" size="icon-sm" disabled={busy} aria-label="Git actions">
              {localActionRunning ? <LoaderCircle className="animate-spin" /> : null}
              {!localActionRunning && localActionSucceeded ? <Check className="text-emerald-500" /> : null}
              {!localActionRunning && !localActionSucceeded ? <MoreHorizontal /> : null}
            </Button>
          }
        />
      </GitToolbarTooltip>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Local</DropdownMenuLabel>
          <DropdownMenuItem onClick={onOpenStashes}>
            <Save />
            Stashes
          </DropdownMenuItem>
          <DropdownMenuItem
            variant="destructive"
            disabled={!canDiscardStaged}
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
            disabled={!canDiscardAll}
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

function GitStashesDialog({
  busy,
  open,
  tabParams,
  onOpenChange,
  onRun,
}: {
  busy: boolean;
  open: boolean;
  tabParams: GitTabParams;
  onOpenChange(open: boolean): void;
  onRun(
    operationId: GitOperationId,
    operation: () => Promise<unknown>,
    options?: { refresh?: boolean; clearCommitMessage?: boolean },
  ): Promise<boolean>;
}) {
  const [listState, setListState] = useState<GitStashListState>({ status: "idle" });
  const [activeStashAction, setActiveStashAction] = useState<ActiveStashAction | null>(null);
  const mountedRef = useRef(true);
  const requestIdRef = useRef(0);

  const loadStashes = async () => {
    if (!mountedRef.current) {
      return;
    }

    const requestId = requestIdRef.current + 1;

    requestIdRef.current = requestId;
    setListState({ status: "loading" });

    try {
      const stashes = await getGitStashes(tabParams);

      if (mountedRef.current && requestIdRef.current === requestId) {
        setListState({ status: "loaded", stashes });
      }
    } catch (caughtError: unknown) {
      if (mountedRef.current && requestIdRef.current === requestId) {
        setListState({ status: "error", message: errorMessage(caughtError) });
      }
    }
  };

  useEffect(() => {
    return () => {
      mountedRef.current = false;
      requestIdRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }

    void loadStashes();
  }, [open, tabParams.workspaceId, tabParams.tabId]);

  const runStashAction = async (
    operationId: "applyStash" | "dropStash",
    selector: string,
    operation: () => Promise<unknown>,
    options?: { refresh?: boolean; clearCommitMessage?: boolean },
  ) => {
    setActiveStashAction({ selector, operationId });

    try {
      const succeeded = await onRun(operationId, operation, options);

      if (succeeded && mountedRef.current) {
        await loadStashes();
      }
    } finally {
      if (mountedRef.current) {
        setActiveStashAction((currentAction) =>
          currentAction?.selector === selector && currentAction.operationId === operationId ? null : currentAction,
        );
      }
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(32rem,calc(100vh-2rem))] max-w-xl overflow-hidden p-0">
        <div className="px-4 pt-4 pr-12">
          <DialogHeader>
            <DialogTitle>Git stashes</DialogTitle>
            <DialogDescription>Apply or remove saved work for this repository.</DialogDescription>
          </DialogHeader>
        </div>

        <div className="min-h-40 overflow-y-auto px-4 pb-4">
          {listState.status === "idle" || listState.status === "loading" ? (
            <GitDialogMessage message="Loading stashes..." />
          ) : null}
          {listState.status === "error" ? <GitDialogMessage message={listState.message} /> : null}
          {listState.status === "loaded" && listState.stashes.length === 0 ? (
            <GitDialogMessage message="No stashes" />
          ) : null}
          {listState.status === "loaded" && listState.stashes.length > 0 ? (
            <ul className="space-y-3">
              {listState.stashes.map((stash) => (
                <GitStashRow
                  key={stash.selector}
                  stash={stash}
                  busy={busy}
                  activeAction={activeStashAction}
                  onApply={() =>
                    void runStashAction("applyStash", stash.selector, () =>
                      applyGitStash({ ...tabParams, selector: stash.selector }),
                    )
                  }
                  onDrop={() => {
                    if (!window.confirm(`Remove ${stash.selector}? This cannot be undone.`)) {
                      return;
                    }

                    void runStashAction(
                      "dropStash",
                      stash.selector,
                      () => dropGitStash({ ...tabParams, selector: stash.selector }),
                      { refresh: false },
                    );
                  }}
                />
              ))}
            </ul>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function GitStashRow({
  activeAction,
  busy,
  stash,
  onApply,
  onDrop,
}: {
  activeAction: ActiveStashAction | null;
  busy: boolean;
  stash: GitStash;
  onApply(): void;
  onDrop(): void;
}) {
  const applying = activeAction?.selector === stash.selector && activeAction.operationId === "applyStash";
  const dropping = activeAction?.selector === stash.selector && activeAction.operationId === "dropStash";
  const details = stashMessageDetails(stash.message);

  return (
    <li>
      <Card size="sm" className="gap-0 bg-background/70 py-0">
        <CardHeader className="items-center border-b px-3 py-2">
          <div className="flex min-w-0 items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2">
              <span className="rounded-md bg-muted px-1.5 py-0.5 font-mono text-[11px] font-medium leading-none text-foreground">
                {stash.selector}
              </span>
              <CardTitle className="truncate text-xs font-medium leading-none">{details.scope}</CardTitle>
            </div>
            <span className="shrink-0 text-xs leading-none text-muted-foreground">
              {formatStashTimestamp(stash.timestamp)}
            </span>
          </div>
        </CardHeader>
        <CardContent className="py-2">
          <p className="break-words text-sm leading-5 text-foreground">{details.summary}</p>
        </CardContent>
        <CardFooter className="justify-end gap-2 bg-muted/30 px-3 py-2">
          <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onApply}>
            {applying ? <LoaderCircle className="animate-spin" /> : <Undo2 />}
            Apply
          </Button>
          <Button type="button" variant="destructive" size="sm" disabled={busy} onClick={onDrop}>
            {dropping ? <LoaderCircle className="animate-spin" /> : <Trash2 />}
            Remove
          </Button>
        </CardFooter>
      </Card>
    </li>
  );
}

function GitDialogMessage({ message }: { message: string }) {
  return (
    <div className="grid min-h-40 place-items-center rounded-lg border border-dashed p-5 text-center">
      <p className="text-sm text-muted-foreground">{message}</p>
    </div>
  );
}

function stashMessageDetails(message: string): { scope: string; summary: string } {
  const normalizedMessage = message.trim();

  if (!normalizedMessage) {
    return { scope: "Stashed changes", summary: "No message" };
  }

  for (const prefix of ["WIP on ", "On "]) {
    if (!normalizedMessage.startsWith(prefix)) {
      continue;
    }

    const rest = normalizedMessage.slice(prefix.length);
    const separatorIndex = rest.indexOf(": ");

    if (separatorIndex > 0) {
      return {
        scope: rest.slice(0, separatorIndex),
        summary: rest.slice(separatorIndex + 2) || "Stashed changes",
      };
    }
  }

  return { scope: "Stashed changes", summary: normalizedMessage };
}

function formatStashTimestamp(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  const options: Intl.DateTimeFormatOptions = {
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    month: "short",
  };

  if (date.getFullYear() !== new Date().getFullYear()) {
    options.year = "numeric";
  }

  return date.toLocaleString(undefined, options);
}

function remoteGitAction(actionId: RemoteGitActionId): RemoteGitAction {
  return REMOTE_GIT_ACTIONS.find((action) => action.id === actionId) ?? REMOTE_GIT_ACTIONS[0]!;
}

function remoteOperationId(actionId: RemoteGitActionId): GitOperationId {
  return `remote:${actionId}`;
}

function isLocalMenuOperation(operation: GitOperationId | null): boolean {
  return operation === "discardStaged" || operation === "discardAll";
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
  if (OPERATION_FEEDBACK_DISABLED.has(operationId)) {
    return <DefaultIcon />;
  }

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
    unsafeCSS: gitTreeCheckboxCss(),
  });

  return (
    <div className="relative h-full min-h-0">
      <PierreFileTree
        model={model}
        className="h-full min-h-0 w-full overflow-hidden bg-card text-card-foreground [--trees-accent-override:var(--accent)] [--trees-bg-muted-override:var(--accent)] [--trees-bg-override:var(--card)] [--trees-border-color-override:var(--border)] [--trees-fg-muted-override:var(--muted-foreground)] [--trees-fg-override:var(--card-foreground)] [--trees-focus-ring-color-override:var(--ring)] [--trees-input-bg-override:var(--input)] [--trees-item-row-gap-override:6px] [--trees-padding-inline-override:0px] [--trees-scrollbar-gutter-override:0px] [--trees-search-bg-override:var(--input)] [--trees-search-fg-override:var(--foreground)] [--trees-selected-bg-override:var(--accent)] [--trees-selected-fg-override:var(--accent-foreground)] [--trees-selected-focused-border-color-override:var(--ring)]"
        style={{ height: "100%" }}
      />
      <GitStageCheckboxOverlay
        model={model}
        changes={changes}
        disabled={disabled}
        onToggleStage={onToggleStage}
      />
    </div>
  );
}

function GitStageCheckboxOverlay({
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
  const overlayRef = useRef<HTMLDivElement>(null);
  const [items, setItems] = useState<StageCheckboxOverlayItem[]>([]);

  useEffect(() => {
    let frameId = 0;
    let cleanup: (() => void) | undefined;
    let mutationObserver: MutationObserver | undefined;
    let resizeObserver: ResizeObserver | undefined;

    const updateItems = () => {
      frameId = 0;

      const overlay = overlayRef.current;
      const shadowRoot = model.getFileTreeContainer()?.shadowRoot;

      if (!overlay || !shadowRoot) {
        return;
      }

      const overlayRect = overlay.getBoundingClientRect();
      const rows = Array.from(shadowRoot.querySelectorAll<HTMLElement>(`[data-type="item"][data-item-path]`));

      setItems(
        rows
          .map((row) => checkboxOverlayItem(row, overlayRect, changes))
          .filter((item): item is StageCheckboxOverlayItem => Boolean(item)),
      );
    };

    const scheduleUpdate = () => {
      if (frameId !== 0) {
        return;
      }

      frameId = requestAnimationFrame(updateItems);
    };

    const attach = () => {
      frameId = 0;
      const container = model.getFileTreeContainer();
      const shadowRoot = container?.shadowRoot;

      if (!container || !shadowRoot || !overlayRef.current) {
        frameId = requestAnimationFrame(attach);
        return;
      }

      shadowRoot.addEventListener("scroll", scheduleUpdate, true);
      window.addEventListener("resize", scheduleUpdate);
      mutationObserver = new MutationObserver(scheduleUpdate);
      mutationObserver.observe(shadowRoot, { attributes: true, childList: true, subtree: true });
      resizeObserver = new ResizeObserver(scheduleUpdate);
      resizeObserver.observe(container);
      scheduleUpdate();

      cleanup = () => {
        shadowRoot.removeEventListener("scroll", scheduleUpdate, true);
        window.removeEventListener("resize", scheduleUpdate);
        mutationObserver?.disconnect();
        resizeObserver?.disconnect();
      };
    };

    frameId = requestAnimationFrame(attach);

    return () => {
      cancelAnimationFrame(frameId);
      cleanup?.();
    };
  }, [model, changes]);

  return (
    <div ref={overlayRef} className="pointer-events-none absolute inset-0 z-10">
      {items.map((item) => (
        <Checkbox
          key={item.path}
          checked={item.state === "checked"}
          indeterminate={item.state === "mixed"}
          disabled={disabled}
          aria-label={`Stage ${item.path}`}
          title={item.title}
          className="pointer-events-auto absolute size-3.5 rounded-[3px] bg-background after:-inset-1 [&>svg]:size-3"
          style={{ left: TREE_CHECKBOX_LEFT, top: item.centerY, transform: "translateY(-50%)" }}
          onCheckedChange={() => onToggleStage(item.paths)}
          onPointerDown={(event) => event.stopPropagation()}
        />
      ))}
    </div>
  );
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

function gitTreeCheckboxCss(): string {
  return `[data-type="item"]{padding-left:24px!important;}`;
}

function checkboxOverlayItem(
  row: HTMLElement,
  overlayRect: DOMRect,
  changes: GitChange[],
): StageCheckboxOverlayItem | null {
  const path = row.dataset.itemPath;

  if (!path) {
    return null;
  }

  const paths = changedPathsForTreePath(changes, path);

  if (paths.length === 0) {
    return null;
  }

  const rowRect = row.getBoundingClientRect();

  return {
    centerY: rowRect.top - overlayRect.top + rowRect.height / 2,
    path,
    paths,
    state: stageStateForTreePath(changes, path),
    title: statusTitle(changes, path),
  };
}

function stageStateForTreePath(changes: GitChange[], path: string): StageCheckboxState {
  const pathChanges = changedPathsForTreePath(changes, path)
    .map((changedPath) => changeForPath(changes, changedPath))
    .filter((change): change is GitChange => Boolean(change));

  if (pathChanges.length === 0) {
    return "unchecked";
  }

  const stagedCount = pathChanges.filter((change) => change.isStaged).length;

  if (stagedCount === 0) {
    return "unchecked";
  }

  return stagedCount === pathChanges.length ? "checked" : "mixed";
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

function GitInitializeRepositoryMessage({
  busy,
  onInitialize,
}: {
  busy: boolean;
  onInitialize(): void;
}) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <div className="flex max-w-sm flex-col items-center gap-3">
        <div className="space-y-1">
          <p className="text-sm font-medium">No Git repository</p>
          <p className="text-sm text-muted-foreground">
            Initialize this workspace to start tracking changes.
          </p>
        </div>
        <Button type="button" disabled={busy} onClick={onInitialize}>
          {busy ? <LoaderCircle className="animate-spin" /> : null}
          Initialize repository
        </Button>
      </div>
    </div>
  );
}

function GitMessage({ message }: { message: string }) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <div className="flex max-w-sm flex-col items-center gap-3">
        <p className="text-sm text-muted-foreground">{message}</p>
      </div>
    </div>
  );
}

function ipcErrorCode(error: unknown): string | undefined {
  if (!error || typeof error !== "object") {
    return undefined;
  }

  const code = "code" in error ? (error as { code?: unknown }).code : undefined;

  if (typeof code === "string") {
    return code;
  }

  if (error instanceof Error) {
    return error.message.match(/\b([a-z]+\.[a-z_]+):/)?.[1];
  }

  return undefined;
}
