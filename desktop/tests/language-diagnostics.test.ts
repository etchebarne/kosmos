import { describe, expect, test } from "bun:test";

import { diagnosticOwners, isCurrentDiagnostics } from "@/renderer/lib/language-diagnostics";

describe("language diagnostics", () => {
  const currentDocument = {
    disposed: false,
    opened: true,
    generation: 12,
    syncedVersion: 4,
    model: { getVersionId: () => 4 },
  };

  test("accepts only the current generation and version", () => {
    expect(isCurrentDiagnostics({ generation: 12, version: 4 }, currentDocument)).toBe(true);
    expect(isCurrentDiagnostics({ generation: 11, version: 4 }, currentDocument)).toBe(false);
    expect(isCurrentDiagnostics({ generation: 12, version: 3 }, currentDocument)).toBe(false);
  });

  test("rejects closed and disposed documents", () => {
    expect(
      isCurrentDiagnostics(
        { generation: 12, version: 4 },
        { ...currentDocument, opened: false },
      ),
    ).toBe(false);
    expect(
      isCurrentDiagnostics(
        { generation: 12, version: 4 },
        { ...currentDocument, disposed: true },
      ),
    ).toBe(false);
  });

  test("recovery preserves each server's marker owner", () => {
    expect(
      diagnosticOwners([
        { serverId: "typescript-language-server" },
        { serverId: "tailwindcss-language-server" },
      ]),
    ).toEqual(["typescript-language-server", "tailwindcss-language-server"]);
  });
});
