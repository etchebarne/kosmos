import { describe, expect, test } from "bun:test";

import {
  createWorkspaceTrustCoordinator,
  type WorkspaceTrustPrompt,
} from "@/renderer/lib/workspace-trust-coordinator";
import { canRetryWorkspaceTrustDocument } from "@/renderer/lib/workspace-trust-retry";

describe("workspace language-server trust", () => {
  test("shares one prompt and authorization request for concurrent opens", async () => {
    const prompts: Array<WorkspaceTrustPrompt | null> = [];
    const coordinator = createWorkspaceTrustCoordinator((prompt) => prompts.push(prompt));
    let authorizations = 0;
    const authorize = async () => {
      authorizations += 1;
      return true;
    };

    const first = coordinator.request(7, authorize);
    const second = coordinator.request(7, authorize);

    expect(first).toBe(second);
    expect(prompts).toEqual([{ workspaceId: 7, isTrusting: false, error: null }]);

    await coordinator.trust(7);

    await expect(first).resolves.toBe("trust");
    await expect(second).resolves.toBe("trust");
    expect(authorizations).toBe(1);
  });

  test("retries only after a successful explicit approval and current-document check", async () => {
    const coordinator = createWorkspaceTrustCoordinator(() => {});
    let retries = 0;
    const decision = coordinator.request(8, async () => true);

    await coordinator.trust(8);
    if (
      (await decision) === "trust" &&
      canRetryWorkspaceTrustDocument({ disposed: false, connectionEpoch: 3 }, 3)
    ) {
      retries += 1;
    }

    expect(retries).toBe(1);
  });

  test("cancellation sends no trust command and does not retry", async () => {
    const coordinator = createWorkspaceTrustCoordinator(() => {});
    let authorizations = 0;
    let retries = 0;
    const decision = coordinator.request(9, async () => {
      authorizations += 1;
      return true;
    });

    coordinator.cancel(9);
    if ((await decision) === "trust") {
      retries += 1;
    }

    expect(authorizations).toBe(0);
    expect(retries).toBe(0);
  });

  test("closing the dialog resolves the safe closed decision", async () => {
    const coordinator = createWorkspaceTrustCoordinator(() => {});
    const decision = coordinator.request(10, async () => true);

    coordinator.close(10);

    await expect(decision).resolves.toBe("closed");
  });

  test("keeps the prompt open with an error when core persistence fails", async () => {
    const prompts: Array<WorkspaceTrustPrompt | null> = [];
    const coordinator = createWorkspaceTrustCoordinator((prompt) => prompts.push(prompt));
    const decision = coordinator.request(11, async () => {
      throw new Error("persistence failed");
    });

    await coordinator.trust(11);

    expect(prompts.at(-1)).toEqual({
      workspaceId: 11,
      isTrusting: false,
      error: "persistence failed",
    });
    coordinator.cancel(11);
    await expect(decision).resolves.toBe("cancel");
  });

  test("does not retry a document disposed while consent was pending", async () => {
    const coordinator = createWorkspaceTrustCoordinator(() => {});
    let retries = 0;
    const decision = coordinator.request(12, async () => true);

    await coordinator.trust(12);
    if (
      (await decision) === "trust" &&
      canRetryWorkspaceTrustDocument({ disposed: true, connectionEpoch: 4 }, 4)
    ) {
      retries += 1;
    }

    expect(retries).toBe(0);
  });
});
