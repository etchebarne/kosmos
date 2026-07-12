import { describe, expect, test } from "bun:test";

import {
  activePrimaryFeatures,
  activeSelectedInstallation,
  activeExternalLanguages,
  documentAttachmentAction,
  documentIsSupported,
  discoverProviderLanguages,
  formatterApplies,
} from "@/renderer/lib/language-client-catalog";
import type { LanguageServerSnapshot } from "@/shared/ipc";

function server(
  id: string,
  languageIds: string[],
  selectedVersion: string | null = "1",
  installedVersion: string | null = selectedVersion,
): LanguageServerSnapshot {
  return {
    id,
    name: id,
    description: "",
    languages: languageIds,
    languageIds,
    catalogVersion: "2",
    selectedVersion,
    installedVersion,
    installationState: "failed",
    lastError: null,
    runtimeState: "running",
    sessionCount: 0,
    workspaceCount: 0,
    runtimeError: null,
    logs: [],
    supported: true,
  };
}

describe("language client catalog", () => {
  test("discovers provider languages once across refreshes", () => {
    const registered = new Set<string>();
    expect(
      discoverProviderLanguages(registered, [
        server("one", ["typescript", "javascript"]),
        server("two", ["typescript"]),
      ]),
    ).toEqual(["typescript", "javascript"]);
    expect(discoverProviderLanguages(registered, [server("three", ["javascript", "json"])]))
      .toEqual(["json"]);
  });

  test("keeps failed old selections active and excludes additive Tailwind overlap", () => {
    const typescript = server("typescript-language-server", ["typescript", "javascript"]);
    const tailwind = server("tailwindcss-language-server", ["typescript", "html"]);
    const features = activePrimaryFeatures([typescript, tailwind]);

    expect(activeSelectedInstallation(typescript)).toBe("1");
    expect(features.get("typescript")).toEqual(
      new Set([
        "completionItems",
        "hovers",
        "signatureHelp",
        "definitions",
        "references",
        "documentSymbols",
        "diagnostics",
        "rename",
        "codeActions",
      ]),
    );
    expect(features.has("html")).toBe(false);
  });

  test("requires the selected installation to be valid before suppressing Monaco", () => {
    const updatingInitialInstall = server(
      "json-language-server",
      ["json"],
      null,
      null,
    );
    const mismatched = server("css-language-server", ["css"], "2", "1");

    expect(activeSelectedInstallation(updatingInitialInstall)).toBeNull();
    expect(activePrimaryFeatures([updatingInitialInstall, mismatched]).size).toBe(0);
  });

  test("restores built-ins and disables external providers when the runtime is unavailable", () => {
    const running = server("typescript-language-server", ["typescript"]);
    const restarting = { ...running, runtimeState: "restarting" as const };
    const crashed = { ...running, runtimeState: "crashed" as const };
    const inactive = { ...running, runtimeState: "inactive" as const };
    const degraded = { ...running, runtimeState: "degraded" as const, sessionCount: 2 };

    expect(activePrimaryFeatures([running]).get("typescript")?.has("rename")).toBe(true);
    expect(activePrimaryFeatures([running]).get("typescript")?.has("codeActions")).toBe(true);
    expect(activePrimaryFeatures([restarting]).has("typescript")).toBe(true);
    expect(activePrimaryFeatures([degraded]).has("typescript")).toBe(true);
    expect(activeExternalLanguages([degraded]).has("typescript")).toBe(true);
    expect(activePrimaryFeatures([crashed, inactive]).size).toBe(0);
    expect(activeExternalLanguages([crashed, inactive]).size).toBe(0);
  });

  test("only installed formatters attach applicable language or path documents", () => {
    const formatter = {
      installedVersion: "1",
      installationState: "installed" as const,
      languageIds: ["typescript"],
      extensions: [".astro"],
      filenames: ["Makefile"],
    };
    expect(formatterApplies(formatter, "typescript", "src/main.ts")).toBe(true);
    expect(formatterApplies(formatter, "plaintext", "src/page.astro")).toBe(true);
    expect(formatterApplies(formatter, "plaintext", "Makefile")).toBe(true);
    expect(formatterApplies({ ...formatter, installedVersion: null }, "typescript", "x.ts")).toBe(false);
    expect(
      formatterApplies({ ...formatter, installationState: "uninstalling" }, "typescript", "x.ts"),
    ).toBe(false);
    expect(documentIsSupported(new Set(), [formatter as never], "plaintext", "page.astro")).toBe(true);
  });

  test("attachment reconciliation is bidirectional", () => {
    expect(documentAttachmentAction(false, true)).toBe("attach");
    expect(documentAttachmentAction(true, false)).toBe("detach");
    expect(documentAttachmentAction(true, true)).toBe("keep");
  });
});
