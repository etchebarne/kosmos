import { PatchDiff, type PatchDiffProps } from "@pierre/diffs/react";
import { useEffect, useRef, useState } from "react";

import { getGitDiff } from "@/renderer/ipc";
import { errorMessage } from "@/renderer/lib/errors";
import { useGitStore } from "@/renderer/stores";
import type { GitDiff, GitDiffFile, TabId, WorkspaceId } from "@/shared/ipc";

type DiffTabProps = {
  workspaceId: WorkspaceId;
  tabId: TabId;
  isActive: boolean;
  onActivatePane(): void;
};

type DiffLoadState =
  | { status: "loading"; workspaceId: WorkspaceId; tabId: TabId }
  | { status: "loaded"; workspaceId: WorkspaceId; tabId: TabId; diff: GitDiff }
  | { status: "error"; workspaceId: WorkspaceId; tabId: TabId; message: string };

const DIFF_REFRESH_INTERVAL_MS = 1500;
const DIFF_THEME_OVERRIDES = `
:host {
  color: var(--foreground);
  background-color: var(--background);
  --diffs-bg: var(--background);
  --diffs-fg: var(--foreground);
  --diffs-fg-number: var(--muted-foreground);
  --diffs-bg-buffer: var(--background);
  --diffs-bg-context: var(--background);
  --diffs-bg-context-gutter: var(--background);
  --diffs-bg-separator: var(--muted);
  --diffs-bg-hover: var(--accent);
  --diffs-addition-base: var(--diff-added);
  --diffs-addition-color: var(--diff-added);
  --diffs-deletion-base: var(--destructive);
  --diffs-deletion-color: var(--destructive);
  --diffs-modified-base: var(--diff-modified);
  --diffs-modified-color: var(--diff-modified);
  --diffs-bg-addition: color-mix(in oklch, var(--background) 88%, var(--diff-added));
  --diffs-bg-addition-number: color-mix(in oklch, var(--background) 76%, var(--diff-added));
  --diffs-bg-addition-hover: color-mix(in oklch, var(--background) 78%, var(--diff-added));
  --diffs-bg-addition-emphasis: color-mix(in oklch, var(--background) 65%, var(--diff-added));
  --diffs-bg-deletion: color-mix(in oklch, var(--background) 88%, var(--destructive));
  --diffs-bg-deletion-number: color-mix(in oklch, var(--background) 76%, var(--destructive));
  --diffs-bg-deletion-hover: color-mix(in oklch, var(--background) 78%, var(--destructive));
  --diffs-bg-deletion-emphasis: color-mix(in oklch, var(--background) 65%, var(--destructive));
}
`;
const DIFF_RENDER_OPTIONS: PatchDiffProps<undefined>["options"] = {
  diffIndicators: "bars",
  diffStyle: "unified",
  hunkSeparators: "line-info",
  lineDiffType: "word-alt",
  overflow: "wrap",
  stickyHeader: true,
  theme: { dark: "pierre-dark", light: "pierre-light" },
  themeType: "dark",
  unsafeCSS: DIFF_THEME_OVERRIDES,
};

export function DiffTab({ workspaceId, tabId, isActive, onActivatePane }: DiffTabProps) {
  const gitRevision = useGitStore((state) => state.revisions[workspaceId] ?? 0);
  const [loadState, setLoadState] = useState<DiffLoadState>({
    status: "loading",
    workspaceId,
    tabId,
  });
  const requestIdRef = useRef(0);
  const revisionRef = useRef(gitRevision);

  const loadDiff = async (targetWorkspaceId: WorkspaceId, targetTabId: TabId, showLoading: boolean) => {
    const requestId = requestIdRef.current + 1;

    requestIdRef.current = requestId;

    if (showLoading) {
      setLoadState({ status: "loading", workspaceId: targetWorkspaceId, tabId: targetTabId });
    }

    try {
      const diff = await getGitDiff({ workspaceId: targetWorkspaceId, tabId: targetTabId });

      if (requestIdRef.current === requestId) {
        setLoadState({ status: "loaded", workspaceId: targetWorkspaceId, tabId: targetTabId, diff });
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
    revisionRef.current = gitRevision;
    void loadDiff(workspaceId, tabId, true);
  }, [workspaceId, tabId]);

  useEffect(() => {
    if (gitRevision === revisionRef.current) {
      return;
    }

    revisionRef.current = gitRevision;
    void loadDiff(workspaceId, tabId, false);
  }, [gitRevision, workspaceId, tabId]);

  useEffect(() => {
    if (!isActive) {
      return undefined;
    }

    const intervalId = window.setInterval(() => {
      void loadDiff(workspaceId, tabId, false);
    }, DIFF_REFRESH_INTERVAL_MS);

    return () => window.clearInterval(intervalId);
  }, [isActive, workspaceId, tabId]);

  const currentLoadState: DiffLoadState =
    loadState.workspaceId === workspaceId && loadState.tabId === tabId
      ? loadState
      : { status: "loading", workspaceId, tabId };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background" onPointerDown={onActivatePane}>
      {currentLoadState.status === "loading" ? <DiffMessage message="Loading diff..." /> : null}
      {currentLoadState.status === "error" ? <DiffMessage message={currentLoadState.message} /> : null}
      {currentLoadState.status === "loaded" ? <LoadedDiff diff={currentLoadState.diff} /> : null}
    </div>
  );
}

function LoadedDiff({ diff }: { diff: GitDiff }) {
  const scrollerRef = useRef<HTMLDivElement>(null);
  const fileRefs = useRef(new Map<string, HTMLElement>());

  useEffect(() => {
    if (!diff.focusedPath) {
      return;
    }

    const frameId = requestAnimationFrame(() => {
      const scroller = scrollerRef.current;
      const file = fileRefs.current.get(diff.focusedPath ?? "");

      if (!scroller || !file) {
        return;
      }

      scroller.scrollTop = file.offsetTop;
    });

    return () => cancelAnimationFrame(frameId);
  }, [diff.focusedPath]);

  const setFileRef = (path: string, node: HTMLElement | null) => {
    if (node) {
      fileRefs.current.set(path, node);
    } else {
      fileRefs.current.delete(path);
    }
  };

  return (
    <div ref={scrollerRef} className="scrollbar-themed h-full min-h-0 min-w-0 overflow-x-hidden overflow-y-auto bg-background">
      {diff.files.length === 0 ? (
        <DiffMessage message="No diff" />
      ) : (
        diff.files.map((file) => <DiffFile key={file.path} file={file} setFileRef={setFileRef} />)
      )}
    </div>
  );
}

function DiffFile({
  file,
  setFileRef,
}: {
  file: GitDiffFile;
  setFileRef(path: string, node: HTMLElement | null): void;
}) {
  const patches = file.sections.flatMap((section) => splitPatchFiles(section.patch));

  return (
    <div ref={(node) => setFileRef(file.path, node)}>
      {patches.map((patch, index) => (
        <PatchDiff
          key={index}
          patch={patch}
          options={DIFF_RENDER_OPTIONS}
          className="block overflow-hidden"
        />
      ))}
    </div>
  );
}

function splitPatchFiles(patch: string): string[] {
  if (patch.trim().length === 0) {
    return [];
  }

  const patches: string[] = [];
  let current: string[] = [];

  for (const line of patch.split(/\r?\n/)) {
    if (line.startsWith("diff --git ") && current.length > 0) {
      patches.push(current.join("\n"));
      current = [];
    }

    current.push(line);
  }

  if (current.length > 0) {
    patches.push(current.join("\n"));
  }

  return patches.filter((patch) => patch.trim().length > 0);
}

function DiffMessage({ message }: { message: string }) {
  return (
    <div className="grid h-full min-h-0 place-items-center overflow-hidden p-5 text-center">
      <p className="text-sm text-muted-foreground">{message}</p>
    </div>
  );
}
