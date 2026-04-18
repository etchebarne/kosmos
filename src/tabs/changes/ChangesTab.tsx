import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PatchDiff } from "@pierre/diffs/react";
import { registerCustomTheme } from "@pierre/diffs";
import { OverlayScrollbarsComponent } from "overlayscrollbars-react";
import { useActiveWorkspace } from "../../contexts/WorkspaceContext";
import { getTheme } from "../../lib/themes";
import { useThemeListener } from "../../hooks/useThemeListener";
import { getChangesMeta } from "../../types";
import { StateView } from "../../components/shared/StateView";
import { isImagePath, joinPath } from "../../lib/pathUtils";
import { ImageViewer } from "../editor/ImageViewer";
import type { TabContentProps } from "../types";

// The callback reads getTheme() lazily so theme swaps apply to new diffs.
let themeRegistered = false;
function ensureTheme() {
  if (themeRegistered) return;
  themeRegistered = true;
  registerCustomTheme("kosmos", async () => {
    const t = getTheme();
    const m = (await import("@pierre/theme/themes/pierre-dark.json")) as {
      default: Record<string, unknown>;
    };
    const base = m.default ?? m;
    return {
      ...base,
      name: "kosmos",
      colors: {
        ...(base.colors as Record<string, string>),
        "editor.background": t.editor.background,
        "editor.foreground": t.editor.foreground,
        "editorLineNumber.foreground": t.editor.lineNumber,
        "editorLineNumber.activeForeground": t.ui.text.tertiary,
      },
    };
  });
}
ensureTheme();

function buildThemeCss(): string {
  const t = getTheme();
  return `
  [data-separator] [data-separator-wrapper] {
    background-color: ${t.ui.bg.surface} !important;
    border-radius: 0 !important;
  }
  [data-separator-content] {
    color: ${t.ui.text.tertiary} !important;
  }
  :host {
    --diffs-bg-buffer-override: ${t.ui.bg.page};
    --diffs-bg-hover-override: ${t.ui.bg.surface};
    --diffs-bg-context-override: ${t.ui.bg.page};
    --diffs-bg-separator-override: ${t.ui.bg.surface};
    --diffs-fg-number-override: ${t.ui.text.muted};
    --diffs-bg-deletion-override: ${t.diff.deletionBg};
    --diffs-bg-deletion-number-override: ${t.diff.deletionNumberBg};
    --diffs-bg-deletion-hover-override: ${t.diff.deletionHoverBg};
    --diffs-bg-deletion-emphasis-override: ${t.diff.deletionEmphasis};
    --diffs-bg-addition-override: ${t.diff.additionBg};
    --diffs-bg-addition-number-override: ${t.diff.additionNumberBg};
    --diffs-bg-addition-hover-override: ${t.diff.additionHoverBg};
    --diffs-bg-addition-emphasis-override: ${t.diff.additionEmphasis};
  }
`;
}

export function ChangesTab({ tab }: TabContentProps) {
  const meta = getChangesMeta(tab);
  const filePath = meta?.filePath ?? "";
  const staged = meta?.staged ?? false;
  const isUntracked = meta?.isUntracked ?? false;

  if (filePath && isImagePath(filePath)) {
    return <ImageChangesView filePath={filePath} />;
  }

  return <DiffChangesView filePath={filePath} staged={staged} isUntracked={isUntracked} />;
}

function ImageChangesView({ filePath }: { filePath: string }) {
  const workspace = useActiveWorkspace();
  if (!workspace?.path) {
    return <StateView message="No workspace" />;
  }
  return <ImageViewer filePath={joinPath(workspace.path, filePath)} />;
}

function DiffChangesView({
  filePath,
  staged,
  isUntracked,
}: {
  filePath: string;
  staged: boolean;
  isUntracked: boolean;
}) {
  const workspace = useActiveWorkspace();
  const [patch, setPatch] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [themeCss, setThemeCss] = useState(buildThemeCss);

  const loadDiff = useCallback(async () => {
    if (!workspace?.path || !filePath) return;
    try {
      let result: string;
      if (isUntracked) {
        result = await invoke<string>("git_diff_untracked", {
          path: workspace.path,
          file: filePath,
        });
      } else {
        result = await invoke<string>("git_diff", {
          path: workspace.path,
          file: filePath,
          staged,
        });
      }
      setPatch(result || "");
      setError(null);
    } catch (e) {
      setError(String(e));
      setPatch(null);
    }
  }, [workspace?.path, filePath, staged, isUntracked]);

  useEffect(() => {
    loadDiff();
  }, [loadDiff]);

  useEffect(() => {
    const unlisten = listen("git-changed", () => {
      loadDiff();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [loadDiff]);

  const handleThemeChanged = useCallback(() => setThemeCss(buildThemeCss()), []);
  useThemeListener(handleThemeChanged);

  if (error) {
    return <StateView message={error} variant="error" />;
  }

  if (patch === null) {
    return <StateView message="Loading diff..." variant="secondary" />;
  }

  if (patch === "") {
    return <StateView message="No changes" />;
  }

  return (
    <OverlayScrollbarsComponent
      className="h-full changes-tab-container"
      options={{
        scrollbars: {
          autoHide: "scroll",
          autoHideDelay: 800,
          theme: "os-theme-custom",
        },
        overflow: { x: "scroll", y: "scroll" },
      }}
    >
      <PatchDiff
        patch={patch}
        options={{
          theme: "kosmos",
          themeType: getTheme().type,
          diffStyle: "unified",
          disableFileHeader: true,
          disableLineNumbers: false,
          overflow: "wrap",
          unsafeCSS: themeCss,
        }}
      />
    </OverlayScrollbarsComponent>
  );
}
