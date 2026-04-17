import { useState, useCallback, useEffect, useRef, useLayoutEffect } from "react";
import { Plus, ArrowsClockwise, Minus, Square, X } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getTheme } from "../../lib/themes";
import { useWorkspaceStore } from "../../store/workspace.store";
import { ContextMenu } from "../shared/ContextMenu";
import { Tooltip } from "../shared/Tooltip";
import { useLayoutStore } from "../../store/layout.store";
import { DRAG_THRESHOLD } from "../../lib/drag-threshold";
import { RemoteDialog } from "./RemoteDialog";
import { TopMenus } from "./TopMenus";
import { useUpdateStore } from "../../store/update.store";

const FLIP_DURATION = 150;
const HEADER_HEIGHT = 36;

export function ProjectBar() {
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const activeIndex = useWorkspaceStore((s) => s.activeIndex);
  const openWorkspace = useWorkspaceStore((s) => s.openWorkspace);
  const switchWorkspace = useWorkspaceStore((s) => s.switchWorkspace);
  const closeWorkspace = useWorkspaceStore((s) => s.closeWorkspace);
  const reorderWorkspace = useWorkspaceStore((s) => s.reorderWorkspace);

  const [dragPath, setDragPath] = useState<string | null>(null);
  const dragRef = useRef({ isDragging: false, currentIndex: 0, swapX: null as number | null });
  const workspaceBtnRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  // Snapshot of button positions taken right before a reorder
  const positionsRef = useRef<Map<string, number>>(new Map());

  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    index: number;
  } | null>(null);

  const [addMenu, setAddMenu] = useState<{ x: number; y: number } | null>(null);
  const [wslDistros, setWslDistros] = useState<string[]>([]);
  const [remoteDialog, setRemoteDialog] = useState<string | null>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);

  // FLIP animation: after React re-renders with new order, animate from old positions
  useLayoutEffect(() => {
    if (positionsRef.current.size === 0) return;
    const oldPositions = positionsRef.current;
    positionsRef.current = new Map();

    workspaceBtnRefs.current.forEach((el, path) => {
      const oldX = oldPositions.get(path);
      if (oldX === undefined) return;
      const newX = el.getBoundingClientRect().left;
      const deltaX = oldX - newX;
      if (Math.abs(deltaX) < 1) return;

      // Invert: jump to old position
      el.style.transition = "none";
      el.style.transform = `translateX(${deltaX}px)`;
      // Play: animate to new position
      requestAnimationFrame(() => {
        el.style.transition = `transform ${FLIP_DURATION}ms cubic-bezier(0.16, 1, 0.3, 1)`;
        el.style.transform = "";
      });
    });
  }, [workspaces]);

  // Fetch WSL distros once
  useEffect(() => {
    invoke<string[]>("list_wsl_distros")
      .then(setWslDistros)
      .catch((e) => console.warn("Failed to list WSL distros:", e));
  }, []);

  // Global Ctrl+P shortcut to open search
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "p") {
        e.preventDefault();
        useLayoutStore.getState().openSearch();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, index: number) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, index });
  }, []);

  const closeMenu = useCallback((menu: "context" | "add") => {
    if (menu === "context") setContextMenu(null);
    else setAddMenu(null);
  }, []);

  const getDropIndex = useCallback((clientX: number) => {
    const refs = workspaceBtnRefs.current;
    const entries = Array.from(refs.entries());
    const state = useWorkspaceStore.getState();
    // Sort entries by current workspace order
    entries.sort((a, b) => {
      const ai = state.workspaces.findIndex((w) => w.path === a[0]);
      const bi = state.workspaces.findIndex((w) => w.path === b[0]);
      return ai - bi;
    });
    for (let i = 0; i < entries.length; i++) {
      const el = entries[i][1];
      const rect = el.getBoundingClientRect();
      const mid = rect.left + rect.width / 2;
      if (clientX < mid) return i;
    }
    return entries.length - 1;
  }, []);

  /** Snapshot all button positions before triggering a reorder. */
  const snapshotPositions = useCallback(() => {
    const snap = new Map<string, number>();
    workspaceBtnRefs.current.forEach((el, path) => {
      snap.set(path, el.getBoundingClientRect().left);
    });
    positionsRef.current = snap;
  }, []);

  const handleWorkspaceMouseDown = useCallback(
    (e: React.MouseEvent, index: number) => {
      if (e.button !== 0) return;
      e.preventDefault();

      const startX = e.clientX;
      const startY = e.clientY;
      const drag = dragRef.current;
      drag.isDragging = false;
      drag.currentIndex = index;
      const path = useWorkspaceStore.getState().workspaces[index]?.path;

      const SWAP_DEAD_ZONE = 16;

      const onMouseMove = (ev: MouseEvent) => {
        const dx = ev.clientX - startX;
        const dy = ev.clientY - startY;
        if (!drag.isDragging && Math.sqrt(dx * dx + dy * dy) > DRAG_THRESHOLD) {
          drag.isDragging = true;
          setDragPath(path);
        }
        if (drag.isDragging) {
          // After a swap, require the cursor to move past a dead zone before allowing another
          if (drag.swapX !== null && Math.abs(ev.clientX - drag.swapX) < SWAP_DEAD_ZONE) {
            return;
          }
          const targetIndex = getDropIndex(ev.clientX);
          if (targetIndex !== drag.currentIndex) {
            snapshotPositions();
            reorderWorkspace(drag.currentIndex, targetIndex);
            drag.currentIndex = targetIndex;
            drag.swapX = ev.clientX;
          }
        }
      };

      const onMouseUp = () => {
        if (!drag.isDragging) {
          switchWorkspace(index);
        }
        drag.isDragging = false;
        drag.swapX = null;
        setDragPath(null);
        document.removeEventListener("mousemove", onMouseMove);
        document.removeEventListener("mouseup", onMouseUp);
      };

      document.addEventListener("mousemove", onMouseMove);
      document.addEventListener("mouseup", onMouseUp);
    },
    [getDropIndex, snapshotPositions, reorderWorkspace, switchWorkspace],
  );

  const handleOpenFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      await openWorkspace(selected);
    }
  };

  const handleAddClick = useCallback(
    (_e: React.MouseEvent) => {
      if (wslDistros.length === 0) {
        // No WSL distros — just open folder directly
        handleOpenFolder();
        return;
      }

      // Show menu with local + WSL options
      const rect = addButtonRef.current?.getBoundingClientRect();
      if (rect) {
        setAddMenu({ x: rect.left, y: rect.bottom + 4 });
      }
    },
    [wslDistros],
  );

  return (
    <div
      data-tauri-drag-region
      className="relative flex items-center bg-[var(--color-bg-surface)] rounded-full overflow-hidden border border-[var(--color-border-primary)]"
      style={{ height: HEADER_HEIGHT, minHeight: HEADER_HEIGHT }}
    >
      {/* ── Left: logo + top menus ── */}
      <div className="flex items-center h-full pl-3 gap-2">
        <KosmosLogo />
        <TopMenus />
      </div>

      {/* spacer for drag region */}
      <div className="flex-1 h-full" data-tauri-drag-region />

      {/* ── Middle: workspaces (absolutely centered) ── */}
      <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 flex items-center gap-1.5">
        {workspaces.map((w, i) => {
          const isActive = i === activeIndex;
          const isDragged = dragPath === w.path;
          return (
            <Tooltip
              key={w.path}
              content={
                w.connection && w.connection.type !== "local"
                  ? `${w.name} [${w.connection.type === "wsl" ? `WSL: ${w.connection.distro}` : w.connection.type === "ssh" ? `SSH: ${w.connection.host}` : ""}]`
                  : w.name
              }
            >
              <button
                ref={(el) => {
                  if (el) {
                    workspaceBtnRefs.current.set(w.path, el);
                  } else {
                    workspaceBtnRefs.current.delete(w.path);
                  }
                }}
                draggable={false}
                className="font-mono w-6 h-6 flex items-center justify-center text-[11px] font-bold shrink-0 hover:opacity-85 overflow-hidden select-none rounded-sm"
                style={{
                  backgroundColor: w.avatarUrl ? undefined : isActive ? w.color : `${w.color}40`,
                  color: isActive ? getTheme().terminal.brightWhite : w.color,
                  opacity: isDragged ? 0.5 : w.avatarUrl && !isActive ? 0.4 : undefined,
                  cursor: dragPath !== null ? "grabbing" : "pointer",
                }}
                onMouseDown={(e) => handleWorkspaceMouseDown(e, i)}
                onContextMenu={(e) => handleContextMenu(e, i)}
              >
                {w.avatarUrl ? (
                  <img
                    src={w.avatarUrl}
                    alt={w.name}
                    draggable={false}
                    className="w-full h-full object-cover pointer-events-none"
                    onError={(e) => {
                      // Hide broken image, show fallback letter
                      (e.target as HTMLImageElement).style.display = "none";
                    }}
                  />
                ) : (
                  w.name[0].toUpperCase()
                )}
              </button>
            </Tooltip>
          );
        })}
        <button
          ref={addButtonRef}
          className="w-6 h-6 flex items-center justify-center border border-[var(--color-border-secondary)] text-[var(--color-text-muted)] shrink-0 hover:border-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)] rounded-sm"
          onClick={handleAddClick}
        >
          <Plus size={12} />
        </button>
      </div>

      {/* spacer for drag region */}
      <div className="flex-1 h-full" data-tauri-drag-region />

      {/* ── Right: update button + window controls ── */}
      <div className="flex items-center h-full">
        <UpdateButton />
        <WindowControls />
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={[
            {
              label: "Close",
              onClick: () => closeWorkspace(contextMenu.index),
            },
          ]}
          onClose={() => closeMenu("context")}
        />
      )}

      {addMenu && (
        <ContextMenu
          x={addMenu.x}
          y={addMenu.y}
          items={[
            { label: "Open Local Folder", onClick: () => handleOpenFolder() },
            { separator: true },
            ...wslDistros.map((distro) => ({
              label: `WSL: ${distro}`,
              onClick: () => setRemoteDialog(distro),
            })),
          ]}
          onClose={() => closeMenu("add")}
        />
      )}

      {remoteDialog && (
        <RemoteDialog open distro={remoteDialog} onClose={() => setRemoteDialog(null)} />
      )}
    </div>
  );
}

function KosmosLogo() {
  // Kosmos logo. Uses currentColor so the theme's --color-logo controls its appearance.
  return (
    <div
      className="flex items-center justify-center shrink-0"
      style={{ color: "var(--color-logo)" }}
      aria-label="Kosmos"
    >
      <svg
        width="17"
        height="14"
        viewBox="0 0 160 133"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <path
          d="M81.6304 43.2982C105.663 49.779 96.2076 69.8639 80.8422 78.1903C104.087 74.6223 121.408 45.2811 100.936 33.3861C67.5553 13.9908 26.4722 44.0913 15.4408 57.1757C-3.27985 79.3803 -5.44038 105.153 11.501 120.616C30.8988 138.321 62.1936 131.983 82.0242 124.581C49.3235 125.375 41.0498 119.824 33.9581 112.686C4.40924 82.9488 47.5986 34.1211 81.6304 43.2982Z"
          fill="currentColor"
        />
        <path
          d="M78.3696 88.3487C54.3366 81.8962 63.7924 61.899 79.1578 53.6089C55.9125 57.1614 38.5923 86.3744 59.0643 98.2175C92.4447 117.528 133.528 87.559 144.559 74.5318C163.28 52.4241 165.44 26.7641 148.499 11.3684C129.101 -6.25952 97.8064 0.0515693 77.9759 7.42062C110.677 6.63073 118.95 12.1575 126.042 19.2634C155.591 48.8712 112.401 97.4857 78.3696 88.3487Z"
          fill="currentColor"
        />
      </svg>
    </div>
  );
}

function WindowControls() {
  const appWindow = getCurrentWindow();
  return (
    <div className="flex items-center ml-1 h-full">
      <button
        className="w-[42px] h-full flex items-center justify-center text-[var(--color-text-muted)] hover:bg-[var(--color-bg-surface)] transition-colors"
        onClick={() => appWindow.minimize()}
      >
        <Minus size={12} />
      </button>
      <button
        className="w-[42px] h-full flex items-center justify-center text-[var(--color-text-muted)] hover:bg-[var(--color-bg-surface)] transition-colors"
        onClick={() => appWindow.toggleMaximize()}
      >
        <Square size={9} weight="bold" />
      </button>
      <button
        className="w-[42px] h-full flex items-center justify-center text-[var(--color-text-muted)] hover:bg-red-500/80 hover:text-white transition-colors"
        onClick={() => appWindow.close()}
      >
        <X size={12} />
      </button>
    </div>
  );
}

function UpdateButton() {
  const update = useUpdateStore((s) => s.update);
  const installing = useUpdateStore((s) => s.installing);
  const installUpdate = useUpdateStore((s) => s.installUpdate);

  if (!update) return null;

  return (
    <Tooltip content={`Update to ${update.version}`}>
      <button
        className="flex items-center gap-1.5 h-7 px-2.5 mr-1 text-[11px] text-[var(--color-accent-blue)] bg-[var(--color-bg-input)] cursor-pointer hover:bg-[var(--color-bg-surface)] transition-colors disabled:opacity-50 disabled:cursor-default"
        onClick={installUpdate}
        disabled={installing}
      >
        <ArrowsClockwise size={12} className={installing ? "animate-spin" : ""} />
        <span>{installing ? "Updating..." : "Update Kosmos"}</span>
      </button>
    </Tooltip>
  );
}
