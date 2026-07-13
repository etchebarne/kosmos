import { describe, expect, test } from "bun:test";

import {
  acceptsToolingRevision,
  documentAttachmentAction,
  discoverProviderLanguages,
  monacoFeatures,
} from "@/renderer/lib/language-client-catalog";
import type { ResolvedToolingDocumentPayload } from "@/shared/ipc";

function document(
  languageId: string,
  features: ResolvedToolingDocumentPayload["features"] = [],
  supported = true,
): ResolvedToolingDocumentPayload {
  return {
    workspaceId: 1,
    path: `src/example.${languageId}`,
    languageId,
    supported,
    externalAvailable: true,
    features,
    formatterId: null,
  };
}

describe("language client capability adapters", () => {
  test("discovers provider languages once from supplied projections", () => {
    const registered = new Set<string>();
    expect(
      discoverProviderLanguages(registered, [
        document("typescript"),
        document("javascript"),
        document("typescript"),
      ]),
    ).toEqual(["typescript", "javascript"]);
    expect(discoverProviderLanguages(registered, [document("javascript"), document("json")]))
      .toEqual(["json"]);
  });

  test("adapts transport-neutral capabilities to Monaco features", () => {
    expect(
      monacoFeatures(document("typescript", [
        { feature: "completion", owners: ["server"] },
        { feature: "navigation", owners: ["server"] },
        { feature: "formatting", owners: ["formatter"] },
      ])),
    ).toEqual(new Set(["completionItems", "definitions", "documentFormattingEdits"]));
  });

  test("does not register providers for unsupported documents", () => {
    const registered = new Set<string>();
    expect(discoverProviderLanguages(registered, [document("plaintext", [], false)])).toEqual([]);
  });

  test("applies only newer capability revisions for an existing document", () => {
    expect(acceptsToolingRevision(undefined, 1)).toBe(true);
    expect(acceptsToolingRevision(4, 5)).toBe(true);
    expect(acceptsToolingRevision(4, 4)).toBe(false);
    expect(acceptsToolingRevision(4, 3)).toBe(false);
  });

  test("attachment reconciliation is bidirectional", () => {
    expect(documentAttachmentAction(false, true)).toBe("attach");
    expect(documentAttachmentAction(true, false)).toBe("detach");
    expect(documentAttachmentAction(true, true)).toBe("keep");
  });
});
