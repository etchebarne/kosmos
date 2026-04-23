import { useCallback, useEffect, useRef, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { editor } from "monaco-editor";
import type { Workspace } from "../../../store/workspace.store";
import { parseDiffChanges, buildDiffDecorations } from "../diffDecorations";

/**
 * Paints git diff indicators (added / modified / deleted) in the editor's line-number
 * gutter. Refreshes on editor/workspace ready and on "git-changed" events.
 */
export function useDiffDecorations(opts: {
  editorRef: RefObject<editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  workspace: Workspace | null;
  filePath: string | undefined;
}): void {
  const { editorRef, editorReady, workspace, filePath } = opts;

  const decorationsRef = useRef<editor.IEditorDecorationsCollection | null>(null);
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const filePathRef = useRef(filePath);
  filePathRef.current = filePath;

  const refresh = useCallback(async () => {
    const ed = editorRef.current;
    const ws = workspaceRef.current;
    const fp = filePathRef.current;
    if (!ed || !ws || !fp) return;

    const relative = fp.startsWith(ws.path + "/") ? fp.slice(ws.path.length + 1) : fp;

    try {
      const patch = await invoke<string>("git_diff", {
        path: ws.path,
        file: relative,
        staged: false,
      });
      const changes = parseDiffChanges(patch);
      const decorations = buildDiffDecorations(changes);
      decorationsRef.current?.clear();
      decorationsRef.current = ed.createDecorationsCollection(decorations);
    } catch {
      // Untracked file: drop stale decorations.
      decorationsRef.current?.clear();
    }
  }, [editorRef]);

  useEffect(() => {
    if (editorReady && workspace) refresh();
  }, [editorReady, workspace, refresh]);

  useEffect(() => {
    const unlisten = listen("git-changed", () => {
      refresh();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);
}
