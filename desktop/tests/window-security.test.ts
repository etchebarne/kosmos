import { describe, expect, test } from "bun:test";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { isSafeExternalUrl, isTrustedRendererUrl } from "@/main/window/security";

describe("window security", () => {
  const rendererEntryPath = path.resolve("dist/renderer/index.html");
  const rendererUrl = pathToFileURL(rendererEntryPath).toString();

  test("trusts only the configured local renderer document", () => {
    expect(isTrustedRendererUrl(rendererUrl, rendererEntryPath)).toBeTrue();
    expect(isTrustedRendererUrl(`${rendererUrl}#settings`, rendererEntryPath)).toBeTrue();
    expect(isTrustedRendererUrl("https://example.com", rendererEntryPath)).toBeFalse();
    expect(
      isTrustedRendererUrl(pathToFileURL(`${rendererEntryPath}.backup`).toString(), rendererEntryPath),
    ).toBeFalse();
  });

  test("opens only HTTP links outside Electron", () => {
    expect(isSafeExternalUrl("https://example.com/docs")).toBeTrue();
    expect(isSafeExternalUrl("http://localhost:3000")).toBeTrue();
    expect(isSafeExternalUrl("file:///etc/passwd")).toBeFalse();
    expect(isSafeExternalUrl("javascript:alert(1)")).toBeFalse();
    expect(isSafeExternalUrl("not a url")).toBeFalse();
  });
});
