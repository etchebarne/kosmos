import { describe, expect, test } from "bun:test";

import { createAsyncQueue } from "@/renderer/lib/async-queue";
import { restoreFormatterPriorityOrder } from "@/renderer/lib/formatter-state";
import type { FormatterSnapshot } from "@/shared/ipc";

function formatter(id: string, installedVersion: string | null): FormatterSnapshot {
  return { id, installedVersion } as FormatterSnapshot;
}

describe("formatter state coordination", () => {
  test("serializes priority and status operations", async () => {
    const queue = createAsyncQueue();
    const order: string[] = [];
    let release!: () => void;
    const first = queue.run(async () => {
      order.push("install:start");
      await new Promise<void>((resolve) => { release = resolve; });
      order.push("install:end");
    });
    const second = queue.run(async () => { order.push("priority"); });

    await Promise.resolve();
    expect(order).toEqual(["install:start"]);
    release();
    await Promise.all([first, second]);
    expect(order).toEqual(["install:start", "install:end", "priority"]);
  });

  test("priority rollback preserves newer formatter status fields", () => {
    const current = [formatter("b", "2"), formatter("a", "1")];
    expect(restoreFormatterPriorityOrder(current, ["a", "b"])).toEqual([
      { ...current[1]!, priority: 0 },
      { ...current[0]!, priority: 1 },
    ]);
  });
});
