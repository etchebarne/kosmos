import { useEffect } from "react";

import { Header } from "@/renderer/components/internal/header";
import { WorkspaceView } from "@/renderer/components/internal/workspace-view";
import { useGitStore, useSettingsStore, useWorkspaceStore } from "@/renderer/stores";

export function App() {
  const initializeWorkspaces = useWorkspaceStore((state) => state.initializeWorkspaces);
  const initializeSettings = useSettingsStore((state) => state.initializeSettings);

  useEffect(() => {
    void initializeWorkspaces();
  }, [initializeWorkspaces]);

  useEffect(() => {
    void initializeSettings();
  }, [initializeSettings]);

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
    </main>
  );
}
