import { useCallback, useRef, useState, forwardRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ContextMenu } from "../shared/ContextMenu";
import { useWorkspaceStore } from "../../store/workspace.store";

type MenuName = "file" | "edit" | "selection";

const MENU_NAMES: MenuName[] = ["file", "edit", "selection"];

/** File / Edit / Selection menubar-style dropdowns. Placeholder items for now. */
export function TopMenus() {
  const openWorkspace = useWorkspaceStore((s) => s.openWorkspace);

  const [openMenu, setOpenMenu] = useState<{ name: MenuName; x: number; y: number } | null>(null);
  const btnRefs = useRef<Record<MenuName, HTMLButtonElement | null>>({
    file: null,
    edit: null,
    selection: null,
  });

  const showMenu = useCallback((name: MenuName) => {
    const el = btnRefs.current[name];
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setOpenMenu({ name, x: rect.left, y: rect.bottom });
  }, []);

  const handleOpenFolder = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) await openWorkspace(selected);
  }, [openWorkspace]);

  const items = (): { label: string; onClick: () => void; disabled?: boolean }[] => {
    // Placeholder items — most are stubs until functionality is wired up.
    switch (openMenu?.name) {
      case "file":
        return [
          { label: "Open Folder...", onClick: () => handleOpenFolder() },
          { label: "Save", onClick: () => {}, disabled: true },
          { label: "Save All", onClick: () => {}, disabled: true },
        ];
      case "edit":
        return [
          { label: "Undo", onClick: () => {}, disabled: true },
          { label: "Redo", onClick: () => {}, disabled: true },
          { label: "Cut", onClick: () => {}, disabled: true },
          { label: "Copy", onClick: () => {}, disabled: true },
          { label: "Paste", onClick: () => {}, disabled: true },
        ];
      case "selection":
        return [
          { label: "Select All", onClick: () => {}, disabled: true },
          { label: "Expand Selection", onClick: () => {}, disabled: true },
          { label: "Shrink Selection", onClick: () => {}, disabled: true },
        ];
      default:
        return [];
    }
  };

  return (
    <div className="flex items-center h-full">
      {MENU_NAMES.map((name) => (
        <TopMenuButton
          key={name}
          ref={(el) => {
            btnRefs.current[name] = el;
          }}
          label={name[0].toUpperCase() + name.slice(1)}
          active={openMenu?.name === name}
          onClick={() => (openMenu?.name === name ? setOpenMenu(null) : showMenu(name))}
          onHover={() => {
            if (openMenu && openMenu.name !== name) showMenu(name);
          }}
        />
      ))}

      {openMenu && (
        <ContextMenu
          x={openMenu.x}
          y={openMenu.y}
          items={items()}
          onClose={() => setOpenMenu(null)}
        />
      )}
    </div>
  );
}

const TopMenuButton = forwardRef<
  HTMLButtonElement,
  {
    label: string;
    active: boolean;
    onClick: () => void;
    onHover: () => void;
  }
>(function TopMenuButton({ label, active, onClick, onHover }, ref) {
  return (
    <button
      ref={ref}
      className={`h-full px-2.5 text-[12px] transition-colors ${
        active
          ? "bg-[var(--color-bg-surface)] text-[var(--color-text-primary)]"
          : "text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-surface)] hover:text-[var(--color-text-primary)]"
      }`}
      // Stop the native mousedown from reaching the document-level listener
      // in ContextMenu's useClickOutside — otherwise clicking an open menu's
      // button closes the menu via the outside-click handler, then the React
      // onClick reopens it.
      onMouseDown={(e) => e.nativeEvent.stopPropagation()}
      onClick={onClick}
      onMouseEnter={onHover}
    >
      {label}
    </button>
  );
});
