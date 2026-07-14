import type {
  FileTree as FileTreeModel,
  FileTreeDirectoryHandle,
  FileTreeItemHandle,
} from "@pierre/trees";

type RefreshableFileTree = Pick<FileTreeModel, "getItem" | "resetPaths">;

export function reconcileFileTreePaths(
  model: RefreshableFileTree,
  previousPaths: readonly string[],
  nextPaths: readonly string[],
): boolean {
  if (stringArraysEqual(previousPaths, nextPaths)) {
    return false;
  }

  const expandedPaths = nextPaths.filter((path) => {
    const item = model.getItem(path);
    return isDirectoryHandle(item) && item.isExpanded();
  });

  model.resetPaths(nextPaths, { initialExpandedPaths: expandedPaths });
  return true;
}

function stringArraysEqual(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function isDirectoryHandle(
  item: FileTreeItemHandle | null,
): item is FileTreeDirectoryHandle {
  return item?.isDirectory() === true;
}
