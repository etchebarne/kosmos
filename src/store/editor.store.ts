import { create } from "zustand";

export const DEFAULT_FONT_SIZE = 13;
export const MIN_FONT_SIZE = 8;
export const MAX_FONT_SIZE = 30;
const FONT_SIZE_STEP = 1;

interface EditorStore {
  editorFontSize: number;
  /**
   * File path of the last editor tab the user interacted with. Used by the
   * top menu (File / Edit / Selection) to know which editor to target.
   * Cleared when that editor's tab is closed.
   */
  lastClickedEditorFilePath: string | null;

  zoomEditorIn: () => void;
  zoomEditorOut: () => void;
  resetEditorZoom: () => void;
  setLastClickedEditor: (filePath: string | null) => void;
}

export const useEditorStore = create<EditorStore>((set) => ({
  editorFontSize: DEFAULT_FONT_SIZE,
  lastClickedEditorFilePath: null,

  zoomEditorIn: () =>
    set((state) => ({
      editorFontSize: Math.min(state.editorFontSize + FONT_SIZE_STEP, MAX_FONT_SIZE),
    })),
  zoomEditorOut: () =>
    set((state) => ({
      editorFontSize: Math.max(state.editorFontSize - FONT_SIZE_STEP, MIN_FONT_SIZE),
    })),
  resetEditorZoom: () => set({ editorFontSize: DEFAULT_FONT_SIZE }),
  setLastClickedEditor: (filePath) => set({ lastClickedEditorFilePath: filePath }),
}));
