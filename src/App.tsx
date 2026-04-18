import { useMemo, useEffect, useLayoutEffect } from "react";
import { ProjectBar } from "./components/layout/ProjectBar";
import { StatusBar } from "./components/layout/StatusBar";
import { EmptyState } from "./components/layout/EmptyState";
import { PaneContainer } from "./components/panes/PaneContainer";
import { PanePortalProvider } from "./components/panes/PanePortalContext";
import { DragOverlay } from "./components/panes/DragOverlay";
import { ToastContainer } from "./components/shared/Toast";
import { WorkspaceProvider } from "./contexts/WorkspaceContext";
import { useShallow } from "zustand/react/shallow";
import { useLayoutStore } from "./store/layout.store";
import { useWorkspaceStore } from "./store/workspace.store";
import { useSettingsStore } from "./store/settings.store";
import { useLspStore } from "./store/lsp.store";
import { useUpdateStore } from "./store/update.store";
import { initPlugins } from "./plugins";
import { applyTheme } from "./lib/themes";
import { prefetch as prefetchFileTree } from "./tabs/fileTree/fileTreeCache";
import "overlayscrollbars/overlayscrollbars.css";
import "./styles/globals.css";

applyTheme("kosmos-dark");

function App() {
  const { layout, layouts, activeWorkspacePath, setWorkspace } = useLayoutStore(
    useShallow((s) => ({
      layout: s.layout,
      layouts: s.layouts,
      activeWorkspacePath: s.activeWorkspacePath,
      setWorkspace: s.setWorkspace,
    })),
  );
  const { connectingPaths, workspaces, activeIndex, ready, init } = useWorkspaceStore(
    useShallow((s) => ({
      connectingPaths: s.connectingPaths,
      workspaces: s.workspaces,
      activeIndex: s.activeIndex,
      ready: s.ready,
      init: s.init,
    })),
  );
  const initSettings = useSettingsStore((s) => s.init);

  useEffect(() => {
    init();
    initSettings();
    initPlugins().catch((err) => console.warn("Plugin init failed:", err));
    useUpdateStore.getState().checkForUpdate();
  }, [init, initSettings]);

  useLayoutEffect(() => {
    if (!ready) return;
    const path = activeIndex !== null ? (workspaces[activeIndex]?.path ?? null) : null;
    if (path) prefetchFileTree(path);
    setWorkspace(path);
  }, [ready, activeIndex, workspaces, setWorkspace]);

  // Warm up LSPs on workspace switch so indexing happens before first open.
  // Depend on size (not the Set identity) so we don't re-run on every render.
  const connectingSize = connectingPaths.size;
  useEffect(() => {
    if (!ready || activeIndex === null) return;
    const activePath = workspaces[activeIndex]?.path;
    if (activePath && !connectingPaths.has(activePath)) {
      useLspStore.getState().warmupWorkspace(activePath);
    }
  }, [ready, activeIndex, workspaces, connectingSize]);

  const allLayouts = useMemo(() => {
    const result = { ...layouts };
    if (activeWorkspacePath) {
      result[activeWorkspacePath] = layout;
    }
    return result;
  }, [layouts, layout, activeWorkspacePath]);

  if (!ready) return null;

  const hasWorkspace = activeIndex !== null;

  return (
    <div
      data-tauri-drag-region
      className="font-ui flex flex-col h-screen w-screen overflow-hidden gap-1.5 p-1.5 bg-[var(--color-bg-page)]"
    >
      <ProjectBar />
      <div className="flex-1 min-h-0 flex rounded-xl overflow-hidden bg-[var(--color-bg-surface)] pill-depth border border-[var(--color-border-primary)]">
        {workspaces.map((ws) => {
          const wsLayout = allLayouts[ws.path];
          if (!wsLayout) return null;
          const isActive = ws.path === activeWorkspacePath;
          const isConnecting = connectingPaths.has(ws.path);
          return (
            <WorkspaceProvider key={ws.path} value={{ workspace: ws, isActive }}>
              <PanePortalProvider layout={wsLayout}>
                <div className={isActive ? "flex w-full h-full min-w-0 min-h-0" : "hidden"}>
                  {isConnecting ? (
                    <div className="flex items-center justify-center w-full h-full">
                      <p className="text-xs text-[var(--color-text-secondary)] animate-pulse">
                        Connecting to remote workspace...
                      </p>
                    </div>
                  ) : (
                    <PaneContainer node={wsLayout} />
                  )}
                </div>
              </PanePortalProvider>
            </WorkspaceProvider>
          );
        })}
        {!hasWorkspace && <EmptyState />}
      </div>
      <StatusBar />
      {hasWorkspace && <DragOverlay />}
      <ToastContainer />
    </div>
  );
}

export default App;
