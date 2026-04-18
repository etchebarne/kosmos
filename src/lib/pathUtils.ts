/** Extract the file name from a file path (handles both `/` and `\`). */
export function getFileName(filePath: string): string {
  const lastSep = Math.max(filePath.lastIndexOf("/"), filePath.lastIndexOf("\\"));
  return lastSep >= 0 ? filePath.substring(lastSep + 1) : filePath;
}

/** Extract the parent directory from a file path. */
export function getParentDir(filePath: string): string {
  const lastSep = Math.max(filePath.lastIndexOf("/"), filePath.lastIndexOf("\\"));
  return lastSep > 0 ? filePath.substring(0, lastSep) : filePath;
}

/** Normalize a path to lowercase with forward slashes (for comparison). */
export function normalizePath(p: string): string {
  return p.replace(/\\/g, "/").toLowerCase();
}

/** Join a directory and name with the correct separator. */
export function joinPath(dir: string, name: string): string {
  const sep = dir.startsWith("wsl://") || dir.includes("/") ? "/" : "\\";
  return dir.endsWith("/") || dir.endsWith("\\") ? dir + name : dir + sep + name;
}

/** Extract a file extension (without dot, lowercased) from a path. Returns null if none. */
export function getFileExtension(filePath: string): string | null {
  return filePath.match(/\.([^./\\]+)$/)?.[1]?.toLowerCase() ?? null;
}
