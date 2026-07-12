import { useEffect } from "react";

import { Header } from "@/renderer/components/internal/header";
import { WorkspaceView } from "@/renderer/components/internal/workspace-view";
import { WorkspaceSymbolPicker } from "@/renderer/components/internal/workspace-symbol-picker";
import { WorkspaceEditRecovery } from "@/renderer/components/internal/workspace-edit-recovery";
import { WorkspaceTrustDialog } from "@/renderer/components/internal/workspace-trust-dialog";
import { UnsavedChangesDialog } from "@/renderer/components/internal/unsaved-changes-dialog";
import { setLanguageLocationOpener } from "@/renderer/lib/language-client";
import { findSetting, useGitStore, useSettingsStore, useWorkspaceStore } from "@/renderer/stores";
import { APPEARANCE_ZOOM_LEVEL } from "@/shared/ipc";

export function App() {
  const initializeWorkspaces = useWorkspaceStore((state) => state.initializeWorkspaces);
  const initializeSettings = useSettingsStore((state) => state.initializeSettings);
  const zoomLevel = useSettingsStore((state) => {
    const value = findSetting(state.snapshot, APPEARANCE_ZOOM_LEVEL)?.value;
    return typeof value === "number" ? value : null;
  });

  useEffect(() => {
    void initializeWorkspaces();
  }, [initializeWorkspaces]);

  useEffect(() => {
    void initializeSettings();
  }, [initializeSettings]);

  useEffect(() => {
    if (zoomLevel !== null) {
      void window.kosmos.setZoomLevel(zoomLevel);
    }
  }, [zoomLevel]);

  useEffect(
    () =>
      window.kosmos.onZoomLevelChanged((nextZoomLevel) => {
        useSettingsStore.getState().updateSetting(APPEARANCE_ZOOM_LEVEL, nextZoomLevel);
      }),
    [],
  );

  useEffect(
    () =>
      window.kosmos.onShutdownRequest(() =>
        useWorkspaceStore.getState().requestApplicationClose(),
      ),
    [],
  );

  useEffect(() => {
    const refresh = () => {
      void useWorkspaceStore.getState().refreshWorkspaces();
      const snapshot = useWorkspaceStore.getState().snapshot;
      if (!snapshot) return;
      for (const workspace of snapshot.workspaces) {
        useGitStore.getState().bumpGitRevision(workspace.id);
      }
    };
    window.addEventListener("kosmos:workspace-edit-applied", refresh);
    return () => window.removeEventListener("kosmos:workspace-edit-applied", refresh);
  }, []);

  useEffect(() => {
    setLanguageLocationOpener((workspaceId, path, selection) =>
      useWorkspaceStore
        .getState()
        .openEditorLocation(
          workspaceId,
          path,
          selection.startLineNumber,
          selection.startColumn,
          selection.endLineNumber,
          selection.endColumn,
        ),
    );
    return () => setLanguageLocationOpener(null);
  }, []);

  useEffect(
    () =>
      window.kosmos.onFlushState(() =>
        useWorkspaceStore.getState().flushPendingState(),
      ),
    [],
  );

  useEffect(
    () =>
      window.kosmos.onWorkspaceChanged((workspaceIds) => {
        const { bumpGitRevision } = useGitStore.getState();

        for (const workspaceId of workspaceIds) {
          bumpGitRevision(workspaceId);
        }
      }),
    [],
  );

  return (
    <main className="flex h-full flex-col gap-2 overflow-hidden bg-muted text-foreground">
      <Header />
      <WorkspaceView />
      <WorkspaceSymbolPicker />
      <WorkspaceEditRecovery />
      <WorkspaceTrustDialog />
      <UnsavedChangesDialog />
    </main>
  );
}
