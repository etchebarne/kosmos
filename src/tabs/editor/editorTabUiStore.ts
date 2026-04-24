import { create } from "zustand";

interface EditorTabUiStore {
  /** Tab ids currently rendering the SVG source (instead of the image preview). */
  svgCodeMode: Set<string>;
  setSvgCodeMode: (tabId: string, codeMode: boolean) => void;
}

export const useEditorTabUiStore = create<EditorTabUiStore>((set) => ({
  svgCodeMode: new Set(),
  setSvgCodeMode: (tabId, codeMode) =>
    set((state) => {
      if (state.svgCodeMode.has(tabId) === codeMode) return state;
      const next = new Set(state.svgCodeMode);
      if (codeMode) next.add(tabId);
      else next.delete(tabId);
      return { svgCodeMode: next };
    }),
}));
