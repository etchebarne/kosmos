import { create } from "zustand";

// ── Selection store ──

interface FileTreeSelectionState {
  selectedPaths: Set<string>;
  anchorPath: string | null;
  select: (path: string) => void;
  rangeSelect: (paths: string[]) => void;
  clear: () => void;
}

export const useFileTreeSelection = create<FileTreeSelectionState>((set) => ({
  selectedPaths: new Set(),
  anchorPath: null,
  select: (path) => set({ selectedPaths: new Set([path]), anchorPath: path }),
  rangeSelect: (paths) =>
    set((state) => ({
      selectedPaths: new Set(paths),
      anchorPath: state.anchorPath,
    })),
  clear: () => set({ selectedPaths: new Set(), anchorPath: null }),
}));

// ── Clipboard store ──

interface FileClipboardState {
  clipboard: {
    mode: "cut" | "copy";
    files: Array<{ path: string; name: string }>;
  } | null;
  set: (clipboard: FileClipboardState["clipboard"]) => void;
  clear: () => void;
}

export const useFileClipboard = create<FileClipboardState>((set) => ({
  clipboard: null,
  set: (clipboard) => set({ clipboard }),
  clear: () => set({ clipboard: null }),
}));
