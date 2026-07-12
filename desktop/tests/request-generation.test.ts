import { describe, expect, test } from "bun:test";

import {
  canConsumeRequest,
  createRequestGeneration,
  isCurrentRequest,
  matchesCurrentQuery,
} from "@/renderer/lib/request-generation";

describe("request generations", () => {
  test("only the latest navigation request may apply state", () => {
    expect(isCurrentRequest(4, 4)).toBe(true);
    expect(isCurrentRequest(3, 4)).toBe(false);
    expect(canConsumeRequest(4, 4)).toBe(true);
    expect(canConsumeRequest(null, 4)).toBe(false);
    expect(canConsumeRequest(3, 4)).toBe(false);
  });

  test("workspace symbol results must match both query and generation", () => {
    const result = { generation: 7, query: "main" };
    expect(matchesCurrentQuery(result, 7, "main")).toBe(true);
    expect(matchesCurrentQuery(result, 8, "main")).toBe(false);
    expect(matchesCurrentQuery(result, 7, "map")).toBe(false);
  });

  test("manual invalidation makes queued navigation stale", () => {
    const requests = createRequestGeneration();
    const queuedNavigation = requests.issue();
    requests.invalidate();
    expect(requests.isCurrent(queuedNavigation)).toBe(false);
  });
});
