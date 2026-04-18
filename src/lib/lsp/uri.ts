/** encodeURI leaves #, ?, etc. intact; re-encode them manually for file URIs. */
function encodeFilePath(path: string): string {
  return encodeURI(path).replace(/[#?;@&=+$,!'()*]/g, (ch) => {
    return `%${ch.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`;
  });
}

/**
 * Convert an OS file path to an LSP-compatible file:// URI.
 * Handles Windows backslashes, drive letters, and percent-encoding.
 */
export function pathToFileUri(path: string): string {
  // Normalize backslashes to forward slashes
  let normalized = path.replace(/\\/g, "/");

  // Ensure drive letter is lowercase for consistency
  if (/^[A-Z]:/.test(normalized)) {
    normalized = normalized[0].toLowerCase() + normalized.slice(1);
  }

  // Percent-encode special characters (spaces, #, ?, etc.)
  normalized = encodeFilePath(normalized);

  // file:///c:/Users/... (Windows) or file:///home/... (Linux)
  if (normalized.startsWith("/")) {
    return `file://${normalized}`;
  }
  return `file:///${normalized}`;
}

/**
 * Convert an LSP file:// URI back to an OS path.
 */
export function fileUriToPath(uri: string): string {
  // Handle both file:/// and file://host/ forms
  let path = uri.replace(/^file:\/\/(?:\/([a-zA-Z]:))?/, "$1");
  if (!path.startsWith("/") && !/^[a-zA-Z]:/.test(path)) {
    // file://host/path -> /path (strip host)
    path = uri.replace(/^file:\/\/[^/]*/, "");
  }

  path = decodeURIComponent(path);

  // After decoding, strip embedded wsl:// prefix: /wsl://distro/path -> wsl://distro/path
  if (path.startsWith("/wsl://")) {
    return path.slice(1);
  }

  // After decoding, handle percent-encoded drive letters (e.g. /c%3A/ decoded to /c:/)
  // Strip the leading / so we get a valid Windows path
  if (/^\/[a-zA-Z]:/.test(path)) {
    path = path.slice(1);
  }

  // On Windows, restore backslashes
  if (/^[a-zA-Z]:/.test(path)) {
    path = path.replace(/\//g, "\\");
  }

  return path;
}
