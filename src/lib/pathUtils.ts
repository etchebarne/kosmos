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

const IMAGE_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
  ico: "image/x-icon",
  bmp: "image/bmp",
  avif: "image/avif",
  apng: "image/apng",
};

/** True if the extension maps to a browser-renderable image format. */
export function isImageExtension(ext: string | null): boolean {
  return ext !== null && ext in IMAGE_MIME;
}

/** True if the path has an image extension we can render. */
export function isImagePath(filePath: string): boolean {
  return isImageExtension(getFileExtension(filePath));
}
