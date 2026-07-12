import { describe, expect, test } from "bun:test";

import {
  isCurrentLanguageResult,
  resolvedWorkspaceSymbolIsCurrent,
} from "../src/renderer/lib/language-feature-adapters";
import type { LanguageServerWorkspaceSymbol } from "../src/shared/ipc";

describe("language feature adapters", () => {
  test("rejects stale, disposed, and cancelled document results", () => {
    const document = {
      disposed: false,
      generation: 12,
      model: { getVersionId: () => 8 },
    };

    expect(isCurrentLanguageResult(document, 12, 8)).toBe(true);
    expect(isCurrentLanguageResult(document, 11, 8)).toBe(false);
    expect(isCurrentLanguageResult(document, 12, 7)).toBe(false);
    expect(isCurrentLanguageResult(document, 12, 8, true)).toBe(false);
    expect(isCurrentLanguageResult({ ...document, disposed: true }, 12, 8)).toBe(false);
  });

  test("workspace symbol resolve cannot change its server identity", () => {
    const source = workspaceSymbol("typescript-language-server", null);
    const resolved = workspaceSymbol("typescript-language-server", {
      workspaceId: 3,
      path: "src/index.ts",
      range: range(),
      selectionRange: range(),
    });

    expect(resolvedWorkspaceSymbolIsCurrent(source, resolved)).toBe(true);
    expect(
      resolvedWorkspaceSymbolIsCurrent(source, {
        ...resolved,
        serverId: "other-server",
      }),
    ).toBe(false);
  });
});

function workspaceSymbol(
  serverId: string,
  location: LanguageServerWorkspaceSymbol["location"],
): LanguageServerWorkspaceSymbol {
  return {
    serverId,
    workspaceId: 3,
    name: "main",
    kind: 12,
    containerName: null,
    deprecated: false,
    location,
    raw: {},
    resolveSupported: true,
  };
}

function range() {
  return {
    start: { line: 0, character: 0 },
    end: { line: 0, character: 4 },
  };
}
