import { useEffect } from "react";

import { Header } from "@/renderer/components/internal/header";
import { WorkspaceView } from "@/renderer/components/internal/workspace-view";
import { useWorkspaceStore } from "@/renderer/stores";

export function App() {
  const initializeWorkspaces = useWorkspaceStore((state) => state.initializeWorkspaces);

  useEffect(() => {
    void initializeWorkspaces();
  }, [initializeWorkspaces]);

  useEffect(
    () =>
      window.kosmos.onFlushState(() =>
        useWorkspaceStore.getState().flushPendingState(),
      ),
    [],
  );

  return (
    <main className="flex h-full flex-col gap-2 overflow-hidden bg-muted text-foreground">
      <Header />
      <WorkspaceView />
    </main>
  );
}
