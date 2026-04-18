import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getFileName, getParentDir, joinPath } from "../../lib/pathUtils";
import type { ContextMenuItem } from "../../components/shared/ContextMenu";
import type { DirEntry } from "./fileTreeTypes";
import { useFileTreeSelection } from "./fileTreeStores";

interface UseFileTreeActionsParams {
  entry: DirEntry;
  depth: number;
  loaded: boolean;
  creating: "file" | "dir" | null;
  clipboard: {
    mode: "cut" | "copy";
    files: Array<{ path: string; name: string }>;
  } | null;
  isSelected: boolean;
  selectionSize: number;
  setChildren: React.Dispatch<React.SetStateAction<DirEntry[]>>;
  setLoaded: React.Dispatch<React.SetStateAction<boolean>>;
  setExpanded: React.Dispatch<React.SetStateAction<boolean>>;
  setCreating: React.Dispatch<React.SetStateAction<"file" | "dir" | null>>;
  setRenaming: React.Dispatch<React.SetStateAction<boolean>>;
  setClipboard: (clipboard: {
    mode: "cut" | "copy";
    files: Array<{ path: string; name: string }>;
  }) => void;
  clearClipboard: () => void;
  refreshDir: (dirPath: string) => void;
}

export function useFileTreeActions({
  entry,
  depth,
  loaded,
  creating,
  clipboard,
  isSelected,
  selectionSize,
  setChildren,
  setLoaded,
  setExpanded,
  setCreating,
  setRenaming,
  setClipboard,
  clearClipboard,
  refreshDir,
}: UseFileTreeActionsParams) {
  const targetDir = entry.isDir ? entry.path : getParentDir(entry.path);

  const handleNewFile = useCallback(() => {
    if (entry.isDir) {
      if (!loaded) {
        invoke<DirEntry[]>("read_dir", { path: entry.path }).then((result) => {
          setChildren(result);
          setLoaded(true);
          setExpanded(true);
          setCreating("file");
        });
      } else {
        setExpanded(true);
        setCreating("file");
      }
    } else {
      window.dispatchEvent(
        new CustomEvent("file-tree-create", {
          detail: { dir: targetDir, type: "file" },
        }),
      );
    }
  }, [entry.isDir, entry.path, loaded, targetDir]);

  const handleNewDir = useCallback(() => {
    if (entry.isDir) {
      if (!loaded) {
        invoke<DirEntry[]>("read_dir", { path: entry.path }).then((result) => {
          setChildren(result);
          setLoaded(true);
          setExpanded(true);
          setCreating("dir");
        });
      } else {
        setExpanded(true);
        setCreating("dir");
      }
    } else {
      window.dispatchEvent(
        new CustomEvent("file-tree-create", {
          detail: { dir: targetDir, type: "dir" },
        }),
      );
    }
  }, [entry.isDir, entry.path, loaded, targetDir]);

  const handleCut = useCallback(() => {
    const sel = useFileTreeSelection.getState().selectedPaths;
    const paths = sel.has(entry.path) && sel.size > 1 ? [...sel] : [entry.path];
    setClipboard({
      mode: "cut",
      files: paths.map((p) => ({ path: p, name: getFileName(p) })),
    });
  }, [entry.path, setClipboard]);

  const handleCopy = useCallback(() => {
    const sel = useFileTreeSelection.getState().selectedPaths;
    const paths = sel.has(entry.path) && sel.size > 1 ? [...sel] : [entry.path];
    setClipboard({
      mode: "copy",
      files: paths.map((p) => ({ path: p, name: getFileName(p) })),
    });
  }, [entry.path, setClipboard]);

  const handlePaste = useCallback(async () => {
    if (!clipboard) return;
    try {
      for (const file of clipboard.files) {
        if (clipboard.mode === "copy") {
          await invoke("copy_entry", {
            source: file.path,
            destDir: targetDir,
          });
        } else {
          await invoke("move_file", {
            source: file.path,
            destDir: targetDir,
          });
        }
      }
      if (clipboard.mode === "cut") clearClipboard();
      refreshDir(targetDir);
    } catch {}
  }, [clipboard, targetDir, refreshDir, clearClipboard]);

  const handleRename = useCallback(
    async (newName: string) => {
      try {
        await invoke("rename_entry", { path: entry.path, newName });
        refreshDir(getParentDir(entry.path));
      } catch {}
      setRenaming(false);
    },
    [entry.path, refreshDir],
  );

  const handleCreate = useCallback(
    async (name: string) => {
      const fullPath = joinPath(entry.path, name);
      try {
        if (creating === "dir") {
          await invoke("create_dir", { path: fullPath });
        } else {
          await invoke("create_file", { path: fullPath });
        }
        refreshDir(entry.path);
      } catch {}
      setCreating(null);
    },
    [entry.path, creating, refreshDir],
  );

  const handleReveal = useCallback(() => {
    invoke("reveal_in_explorer", { path: entry.path });
  }, [entry.path]);

  const handleTrash = useCallback(async () => {
    const sel = useFileTreeSelection.getState().selectedPaths;
    const paths = sel.has(entry.path) && sel.size > 1 ? [...sel] : [entry.path];
    const dirsToRefresh = new Set<string>();
    for (const p of paths) {
      try {
        await invoke("trash_entry", { path: p });
        dirsToRefresh.add(getParentDir(p));
      } catch {}
    }
    for (const d of dirsToRefresh) refreshDir(d);
    useFileTreeSelection.getState().clear();
  }, [entry.path, refreshDir]);

  const handleDelete = useCallback(async () => {
    const sel = useFileTreeSelection.getState().selectedPaths;
    const paths = sel.has(entry.path) && sel.size > 1 ? [...sel] : [entry.path];
    const dirsToRefresh = new Set<string>();
    for (const p of paths) {
      try {
        await invoke("delete_entry", { path: p });
        dirsToRefresh.add(getParentDir(p));
      } catch {}
    }
    for (const d of dirsToRefresh) refreshDir(d);
    useFileTreeSelection.getState().clear();
  }, [entry.path, refreshDir]);

  const multiSelected = isSelected && selectionSize > 1;
  const isRoot = depth === 0;
  const contextMenuItems: ContextMenuItem[] = isRoot
    ? [
        { label: "New File", onClick: handleNewFile },
        { label: "New Folder", onClick: handleNewDir },
        { separator: true },
        {
          label: "Paste",
          onClick: handlePaste,
          disabled: !clipboard,
        },
        { separator: true },
        { label: "Reveal in File Explorer", onClick: handleReveal },
      ]
    : [
        { label: "New File", onClick: handleNewFile },
        { label: "New Folder", onClick: handleNewDir },
        { separator: true },
        { label: "Cut", onClick: handleCut },
        { label: "Copy", onClick: handleCopy },
        {
          label: "Paste",
          onClick: handlePaste,
          disabled: !clipboard,
        },
        { separator: true },
        {
          label: "Rename",
          onClick: () => setRenaming(true),
          disabled: multiSelected,
        },
        { separator: true },
        { label: "Reveal in File Explorer", onClick: handleReveal },
        { separator: true },
        { label: "Move to Trash", onClick: handleTrash, destructive: true },
        { label: "Delete", onClick: handleDelete, destructive: true },
      ];

  return {
    handleNewFile,
    handleNewDir,
    handleCut,
    handleCopy,
    handlePaste,
    handleRename,
    handleCreate,
    handleReveal,
    handleTrash,
    handleDelete,
    contextMenuItems,
  };
}
