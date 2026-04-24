import { findLeaf } from "../lib/paneTree";
import { useLayoutStore } from "../store/layout.store";

export function useIsTabActive(paneId: string, tabId: string): boolean {
  return useLayoutStore((s) => {
    const leaf = findLeaf(s.layout, paneId);
    return leaf?.activeTabId === tabId;
  });
}
