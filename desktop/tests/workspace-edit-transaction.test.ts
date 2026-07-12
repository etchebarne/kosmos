import { describe, expect, test } from "bun:test";

import type { StagedWorkspaceEdit } from "../src/shared/ipc/language-servers";
import {
  applyWorkspaceEditTransaction,
  finalizeWorkspaceEditRecovery,
  retryWorkspaceEditRecovery,
  replaceWorkspaceEditModel,
  workspaceEditRecoveryIds,
  type OpenWorkspaceEditTarget,
  type WorkspaceEditTransactionAdapter,
} from "../src/renderer/lib/workspace-edit-transaction";

function edit(): StagedWorkspaceEdit {
  return {
    transactionId: 7,
    authorization: "test-authorization",
    documents: [
      {
        workspaceId: 1,
        path: "open.ts",
        originalText: "before",
        newText: "after",
        generation: 3,
        version: 5,
      },
    ],
  };
}

describe("workspace edit transaction coordinator", () => {
  test("commits, applies, and always finishes successful edits", async () => {
    const events: string[] = [];
    await applyWorkspaceEditTransaction(edit(), adapter(events));
    expect(events).toEqual([
      "preflight",
      "commit",
      "revalidate",
      "validate",
      "apply",
      "finish",
    ]);
  });

  test("preflight and commit failures finish without rolling back uncommitted files", async () => {
    const events: string[] = [];
    const failing = adapter(events);
    failing.preflight = () => {
      events.push("preflight");
      throw new Error("stale model");
    };
    await expect(applyWorkspaceEditTransaction(edit(), failing)).rejects.toThrow("stale model");
    expect(events).toEqual(["preflight", "finish"]);

    events.length = 0;
    const commitFailure = adapter(events);
    commitFailure.commitClosed = async () => {
      events.push("commit");
      throw new Error("stale hash");
    };
    await expect(applyWorkspaceEditTransaction(edit(), commitFailure)).rejects.toThrow("stale hash");
    expect(events).toEqual(["preflight", "commit", "finish"]);
  });

  test("partial renderer failure undoes applied models and rolls back closed files", async () => {
    const events: string[] = [];
    const staged = edit();
    staged.documents.push({ ...staged.documents[0]!, path: "second.ts" });
    let index = 0;
    const failing = adapter(events);
    failing.preflight = () => {
      events.push(`preflight:${index}`);
      const current = index++;
        return {
          document: staged.documents[current]!,
          validate() {
            events.push(`validate:${current}`);
          },
          apply() {
          events.push(`apply:${current}`);
          if (current === 1) {
            throw new Error("Monaco rejected edits");
          }
        },
        undo() {
          events.push(`undo:${current}`);
        },
      };
    };
    await expect(applyWorkspaceEditTransaction(staged, failing)).rejects.toThrow(
      "Monaco rejected edits",
    );
    expect(events).toEqual([
      "preflight:0",
      "preflight:1",
      "commit",
      "revalidate",
      "revalidate",
      "validate:0",
      "apply:0",
      "validate:1",
      "apply:1",
      "undo:1",
      "undo:0",
      "rollback",
      "finish",
    ]);
  });

  test("revalidates every target after commit and rolls back before applying any model", async () => {
    const events: string[] = [];
    const stale = adapter(events);
    stale.validate = () => {
      events.push("revalidate");
      throw new Error("model changed during commit");
    };
    await expect(applyWorkspaceEditTransaction(edit(), stale)).rejects.toThrow(
      "model changed during commit",
    );
    expect(events).toEqual(["preflight", "commit", "revalidate", "rollback", "finish"]);
  });

  test("cancellation after closed commit synchronizes rollback before finish", async () => {
    const events: string[] = [];
    const cancellation = new AbortController();
    const current = adapter(events);
    current.commitClosed = async () => {
      events.push("commit");
      cancellation.abort(new Error("server cancelled"));
    };
    await expect(
      applyWorkspaceEditTransaction(edit(), current, cancellation.signal),
    ).rejects.toThrow("server cancelled");
    expect(events).toEqual(["preflight", "commit", "rollback", "finish"]);
  });

  test("recovers a lost commit response by querying and rolling back", async () => {
    const events: string[] = [];
    const current = responseLossAdapter(events, "commit");
    await expect(applyWorkspaceEditTransaction(edit(), current)).rejects.toThrow("response lost");
    expect(events).toEqual(["preflight", "commit", "rollback", "finish"]);
  });

  test("treats a lost finish response as success after durable completion", async () => {
    const events: string[] = [];
    await applyWorkspaceEditTransaction(edit(), responseLossAdapter(events, "finish"));
    expect(events).toEqual([
      "preflight",
      "commit",
      "revalidate",
      "validate",
      "apply",
      "finish",
    ]);
  });

  test("retains failed recovery until retry succeeds", async () => {
    const events: string[] = [];
    let undoFails = true;
    const current = adapter(events);
    current.preflight = (document) => ({
      document,
      validate() {
        events.push("validate");
      },
      apply() {
        events.push("apply");
        throw new Error("apply failed");
      },
      undo() {
        events.push("undo");
        if (undoFails) {
          throw new Error("undo failed");
        }
      },
    });
    await expect(applyWorkspaceEditTransaction(edit(), current)).rejects.toThrow(
      "Retry recovery",
    );
    expect(workspaceEditRecoveryIds()).toContain(7);
    expect(events).not.toContain("finish");
    undoFails = false;
    await retryWorkspaceEditRecovery(7);
    expect(workspaceEditRecoveryIds()).not.toContain(7);
    expect(events.at(-1)).toBe("finish");
  });

  test("partial closed-file commit recovery failure is retained and never finished", async () => {
    const events: string[] = [];
    const current = adapter(events);
    const recoveryError = Object.assign(new Error("partial commit"), {
      code: "workspace_edit.recovery_required",
    });
    current.commitClosed = async () => {
      events.push("commit");
      throw recoveryError;
    };
    current.rollbackClosed = async () => {
      events.push("rollback");
      throw new Error("rollback still blocked");
    };
    current.isRecoveryRequired = (error) => error === recoveryError;
    const staged = edit();
    staged.transactionId = 8;
    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "Recovery is incomplete",
    );
    expect(events).toEqual(["preflight", "commit", "rollback"]);
    expect(workspaceEditRecoveryIds()).toContain(8);
    await finalizeWorkspaceEditRecovery(8);
    expect(workspaceEditRecoveryIds()).not.toContain(8);
  });

  test("workspace edits clear ordinary per-model undo instead of allowing partial undo", () => {
    const ordinaryUndo: string[] = ["user edit"];
    let value = "before";
    const model = {
      getValue: () => value,
      setValue(next: string) {
        value = next;
        ordinaryUndo.length = 0;
      },
    };
    replaceWorkspaceEditModel(model, "after");
    expect(value).toBe("after");
    expect(ordinaryUndo).toEqual([]);
  });
});

function responseLossAdapter(
  events: string[],
  lostAt: "commit" | "finish",
): WorkspaceEditTransactionAdapter {
  let phase: "staged" | "committed" | "rolledBack" | "finishedCommitted" | "finishedRolledBack" = "staged";
  return {
    validate() {
      events.push("revalidate");
    },
    preflight(document) {
      events.push("preflight");
      return {
        document,
        validate() {
          events.push("validate");
        },
        apply() {
          events.push("apply");
        },
        undo() {
          events.push("undo");
        },
      };
    },
    async commitClosed() {
      events.push("commit");
      phase = "committed";
      if (lostAt === "commit") {
        throw new Error("response lost");
      }
    },
    async rollbackClosed() {
      events.push("rollback");
      phase = "rolledBack";
    },
    async finish() {
      events.push("finish");
      phase = phase === "committed" ? "finishedCommitted" : "finishedRolledBack";
      if (lostAt === "finish") {
        throw new Error("response lost");
      }
    },
    async finalize() {},
    async status(transactionId) {
      return { transactionId, phase, retryRollback: false, canFinalize: false };
    },
  };
}

function adapter(events: string[]): WorkspaceEditTransactionAdapter {
  let phase: Awaited<ReturnType<WorkspaceEditTransactionAdapter["status"]>>["phase"] = "staged";
  const target: OpenWorkspaceEditTarget = {
    document: edit().documents[0]!,
    validate() {
      events.push("validate");
    },
    apply() {
      events.push("apply");
    },
    undo() {
      events.push("undo");
    },
  };
  return {
    validate() {
      events.push("revalidate");
    },
    preflight() {
      events.push("preflight");
      return target;
    },
    async commitClosed() {
      events.push("commit");
      phase = "committed";
    },
    async rollbackClosed() {
      events.push("rollback");
      phase = "rolledBack";
    },
    async finish() {
      events.push("finish");
      phase = phase === "committed"
        ? "finishedCommitted"
        : phase === "rolledBack"
          ? "finishedRolledBack"
          : "finishedUncommitted";
    },
    async finalize() {
      events.push("finish");
      phase = "finishedRolledBack";
    },
    async status(transactionId) {
      return {
        transactionId,
        phase,
        retryRollback: phase === "recoveryRequired",
        canFinalize: phase === "recoveryRequired",
      };
    },
  };
}
