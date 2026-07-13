import path from "node:path";
import { fileURLToPath } from "node:url";

export function isTrustedRendererUrl(url: string, rendererEntryPath: string): boolean {
  try {
    const parsed = new URL(url);
    return (
      parsed.protocol === "file:" &&
      path.resolve(fileURLToPath(parsed)) === path.resolve(rendererEntryPath)
    );
  } catch {
    return false;
  }
}

export function isSafeExternalUrl(url: string): boolean {
  try {
    const protocol = new URL(url).protocol;
    return protocol === "https:" || protocol === "http:";
  } catch {
    return false;
  }
}
