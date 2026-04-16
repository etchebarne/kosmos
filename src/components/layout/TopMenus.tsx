import { useCallback, useRef, useState, forwardRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { ContextMenu } from "../shared/ContextMenu";
import { useWorkspaceStore } from "../../store/workspace.store";
import { useEditorStore } from "../../store/editor.store";
import { useLayoutStore } from "../../store/layout.store";
import { editorCache } from "../../tabs/editor/editor-cache";

type MenuName = "file" | "edit" | "selection";

const MENU_NAMES: MenuName[] = ["file", "edit", "selection"];

/**
 * File / Edit / Selection menubar-style dropdowns.
 *
 * Every action except "Open Folder..." / "Save All" targets the last editor
 * tab the user interacted with (tracked via `useEditorStore.lastClickedEditorFilePath`).
 * The editor instance is resolved through `editorCache`, keyed by file path.
 */
export function TopMenus() {
  const openWorkspace = useWorkspaceStore((s) => s.openWorkspace);
  const lastClickedEditorFilePath = useEditorStore((s) => s.lastClickedEditorFilePath);
  const dirtyTabs = useLayoutStore((s) => s.dirtyTabs);

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

  const getTargetEditor = useCallback(() => {
    if (!lastClickedEditorFilePath) return null;
    const entry = editorCache.get(lastClickedEditorFilePath);
    return entry?.instance ?? null;
  }, [lastClickedEditorFilePath]);

  const runOnEditor = useCallback(
    (fn: (ed: NonNullable<ReturnType<typeof getTargetEditor>>) => void) => {
      const ed = getTargetEditor();
      if (!ed) return;
      fn(ed);
      ed.focus();
    },
    [getTargetEditor],
  );

  const handleSave = useCallback(async () => {
    if (!lastClickedEditorFilePath) return;
    const entry = editorCache.get(lastClickedEditorFilePath);
    await entry?.save?.();
  }, [lastClickedEditorFilePath]);

  const handleSaveAll = useCallback(async () => {
    await Promise.all(
      Array.from(editorCache.values())
        .filter((e) => e.instance && e.save)
        .map((e) => e.save!()),
    );
  }, []);

  const handleUndo = useCallback(() => {
    runOnEditor((ed) => ed.trigger("top-menu", "undo", null));
  }, [runOnEditor]);

  const handleRedo = useCallback(() => {
    runOnEditor((ed) => ed.trigger("top-menu", "redo", null));
  }, [runOnEditor]);

  const handleCut = useCallback(() => {
    runOnEditor((ed) => {
      const sel = ed.getSelection();
      if (!sel || sel.isEmpty()) return;
      const text = ed.getModel()?.getValueInRange(sel) ?? "";
      navigator.clipboard.writeText(text);
      ed.executeEdits("top-menu", [{ range: sel, text: "" }]);
    });
  }, [runOnEditor]);

  const handleCopy = useCallback(() => {
    runOnEditor((ed) => {
      const sel = ed.getSelection();
      if (!sel || sel.isEmpty()) return;
      navigator.clipboard.writeText(ed.getModel()?.getValueInRange(sel) ?? "");
    });
  }, [runOnEditor]);

  const handlePaste = useCallback(async () => {
    const ed = getTargetEditor();
    if (!ed) return;
    try {
      const text = await readText();
      if (text) ed.trigger("top-menu", "type", { text });
    } catch {
      /* clipboard empty or inaccessible */
    }
    ed.focus();
  }, [getTargetEditor]);

  const handleSelectAll = useCallback(() => {
    runOnEditor((ed) => {
      const model = ed.getModel();
      if (!model) return;
      const lastLine = model.getLineCount();
      const lastCol = model.getLineMaxColumn(lastLine);
      ed.setSelection({
        startLineNumber: 1,
        startColumn: 1,
        endLineNumber: lastLine,
        endColumn: lastCol,
      });
    });
  }, [runOnEditor]);

  const handleExpandSelection = useCallback(() => {
    runOnEditor((ed) => {
      ed.getAction("editor.action.smartSelect.expand")?.run();
    });
  }, [runOnEditor]);

  const handleShrinkSelection = useCallback(() => {
    runOnEditor((ed) => {
      ed.getAction("editor.action.smartSelect.shrink")?.run();
    });
  }, [runOnEditor]);

  const items = (): { label: string; onClick: () => void; disabled?: boolean }[] => {
    const hasTarget = !!getTargetEditor();
    const sel = getTargetEditor()?.getSelection();
    const hasSelection = !!sel && !sel.isEmpty();
    const lastPath = lastClickedEditorFilePath;
    const currentTabDirty = (() => {
      if (!lastPath) return false;
      // Find the tab whose filePath matches; cheap enough walk over dirtyTabs + layout.
      const { layout } = useLayoutStore.getState();
      const stack: (typeof layout)[] = [layout];
      while (stack.length) {
        const n = stack.pop()!;
        if (n.type === "leaf") {
          for (const t of n.tabs) {
            if (t.type === "editor" && (t.metadata?.filePath as string) === lastPath) {
              return dirtyTabs.has(t.id);
            }
          }
        } else {
          stack.push(...n.children);
        }
      }
      return false;
    })();
    const anyDirty = dirtyTabs.size > 0;

    switch (openMenu?.name) {
      case "file":
        return [
          { label: "Open Folder...", onClick: () => handleOpenFolder() },
          { label: "Save", onClick: () => handleSave(), disabled: !hasTarget || !currentTabDirty },
          { label: "Save All", onClick: () => handleSaveAll(), disabled: !anyDirty },
        ];
      case "edit":
        return [
          { label: "Undo", onClick: handleUndo, disabled: !hasTarget },
          { label: "Redo", onClick: handleRedo, disabled: !hasTarget },
          { label: "Cut", onClick: handleCut, disabled: !hasSelection },
          { label: "Copy", onClick: handleCopy, disabled: !hasSelection },
          { label: "Paste", onClick: handlePaste, disabled: !hasTarget },
        ];
      case "selection":
        return [
          { label: "Select All", onClick: handleSelectAll, disabled: !hasTarget },
          { label: "Expand Selection", onClick: handleExpandSelection, disabled: !hasTarget },
          { label: "Shrink Selection", onClick: handleShrinkSelection, disabled: !hasTarget },
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
      // When clicking this button while its menu is open, ContextMenu's
      // capture-phase useClickOutside already closes the menu at mousedown.
      // `active` in this onClick closure reflects the state at click dispatch
      // (React batches state updates across a user event), so if it's true
      // the user's intent — "close the menu" — is already fulfilled. Skipping
      // `onClick()` here prevents the close-then-reopen flicker.
      onClick={() => {
        if (active) return;
        onClick();
      }}
      onMouseEnter={onHover}
    >
      {label}
    </button>
  );
});
