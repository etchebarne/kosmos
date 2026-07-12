import { expect, test } from "bun:test";

import { createQueuedRefresh } from "@/renderer/lib/queued-refresh";

test("a refresh requested in flight runs once more before callers settle", async () => {
  const releases: Array<() => void> = [];
  let runs = 0;
  const refresh = createQueuedRefresh(async () => {
    runs += 1;
    await new Promise<void>((resolve) => releases.push(resolve));
  });

  const first = refresh();
  const second = refresh();
  expect(first).toBe(second);
  expect(runs).toBe(1);
  releases.shift()?.();
  await Promise.resolve();
  await Promise.resolve();
  expect(runs).toBe(2);
  releases.shift()?.();
  await first;
  expect(runs).toBe(2);
});
