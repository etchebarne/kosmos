import { describe, expect, test } from "bun:test";

import { loadBootstrapSettings } from "@/main/settings-snapshot";
import { startWithFatalHandler } from "@/main/startup";

describe("startup", () => {
  test("routes an invalid bootstrap settings snapshot to the fatal startup handler", async () => {
    const failures: unknown[] = [];

    await startWithFatalHandler(
      async () => {
        await loadBootstrapSettings(async () => ({}));
      },
      (error) => failures.push(error),
    );

    expect(failures).toHaveLength(1);
    expect(failures[0]).toBeInstanceOf(Error);
    expect((failures[0] as Error).message).toContain("Invalid settings.get result");
  });
});
