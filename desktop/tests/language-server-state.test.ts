import { describe, expect, test } from "bun:test";

import {
  hasRunningLanguageServer,
  languageServerOperationInProgress,
  pendingServersAfterStatus,
  statusRetryDelay,
} from "@/renderer/lib/language-server-state";
import type { LanguageServerSnapshot } from "@/shared/ipc";

const server = {
  id: "typescript-language-server",
  installationState: "installed",
  runtimeState: "running",
} as LanguageServerSnapshot;

describe("language server status recovery", () => {
  test("pending state follows installation and restart status", () => {
    const restarting = { ...server, runtimeState: "restarting" as const };
    expect(languageServerOperationInProgress(restarting)).toBe(true);
    expect(pendingServersAfterStatus({}, restarting)).toEqual({ [server.id]: true });
    expect(pendingServersAfterStatus({ [server.id]: true }, server)).toEqual({});
  });

  test("retry backoff is bounded", () => {
    expect(statusRetryDelay(0)).toBe(250);
    expect(statusRetryDelay(20)).toBe(4_000);
  });

  test("a language server counts as running only with an active session", () => {
    expect(hasRunningLanguageServer([])).toBe(false);
    expect(hasRunningLanguageServer([{ ...server, sessionCount: 0 }])).toBe(false);
    expect(hasRunningLanguageServer([{ ...server, sessionCount: 1, runtimeState: "crashed" }])).toBe(false);
    expect(hasRunningLanguageServer([{ ...server, sessionCount: 1 }])).toBe(true);
    expect(hasRunningLanguageServer([{ ...server, sessionCount: 2, runtimeState: "degraded" }])).toBe(true);
  });
});
