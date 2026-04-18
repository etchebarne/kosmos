import type { TabDefinition } from "../types";
import { FileTreeTab } from "./FileTreeTab";

export const fileTreeTab: TabDefinition = {
  type: "file-tree",
  title: "File Tree",
  icon: "folder-tree",
  component: FileTreeTab,
  defaultSize: { width: 360, height: 500 },
};
