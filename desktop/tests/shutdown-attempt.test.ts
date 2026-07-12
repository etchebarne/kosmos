import { describe, expect, test } from "bun:test";

import { createShutdownAttempt } from "@/main/shutdown-attempt";

describe("shutdown attempt", () => {
  test("a cancelled dirty-document decision keeps the sidecar running", async () => {
    let stopped = 0;
    const attempt = createShutdownAttempt(async () => {
      stopped += 1;
    });

    expect(await attempt.attempt(async () => false)).toBe("cancelled");
    expect(stopped).toBe(0);
    expect(attempt.complete).toBeFalse();
  });

  test("a failed first attempt is rearmed for a successful second attempt", async () => {
    let calls = 0;
    const attempt = createShutdownAttempt(async () => {
      calls += 1;
      if (calls === 1) throw new Error("timed out");
    });

    expect(await attempt.attempt(async () => true)).toBe("failed");
    expect(attempt.complete).toBeFalse();
    expect(await attempt.attempt(async () => true)).toBe("completed");
    expect(calls).toBe(2);
    expect(attempt.complete).toBeTrue();
  });
});
