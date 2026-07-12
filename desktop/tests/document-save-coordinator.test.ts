import { describe, expect, test } from "bun:test";

import { createDocumentSaveCoordinator } from "@/renderer/lib/document-save-coordinator";

describe("document save coordination", () => {
  test("starts every safety snapshot immediately and suppresses an older formatted write", async () => {
    const coordinator = createDocumentSaveCoordinator();
    const events: string[] = [];
    let releaseFirst!: () => void;
    const firstSnapshot = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });

    const first = coordinator.begin(async () => {
      events.push("snapshot:first");
      await firstSnapshot;
    });
    const firstCompletion = first.run(async () => {
      events.push("formatted-write:first");
    });
    const second = coordinator.begin(async () => {
      events.push("snapshot:second");
    });
    const secondCompletion = second.run(async () => {
      events.push("formatted-write:second");
    });

    expect(events).toEqual(["snapshot:first", "snapshot:second"]);
    releaseFirst();
    await Promise.all([firstCompletion, secondCompletion]);
    expect(events).toEqual([
      "snapshot:first",
      "snapshot:second",
      "formatted-write:second",
    ]);
  });
});
