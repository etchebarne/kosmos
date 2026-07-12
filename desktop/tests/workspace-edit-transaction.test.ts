import { describe, expect, test } from "bun:test";

import type { StagedWorkspaceEdit } from "../src/shared/ipc";
import {
  applyWorkspaceEditTransaction,
  finalizeWorkspaceEditRecovery,
  registerPersistedWorkspaceEditRecoveries,
  retryWorkspaceEditRecovery,
  replaceWorkspaceEditModel,
  workspaceEditRecoveryActions,
  workspaceEditRecoveryIds,
  workspaceEditRecoveryWarnings,
  dismissWorkspaceEditRecoveryWarning,
  type OpenWorkspaceEditTarget,
  type WorkspaceEditTransactionAdapter,
} from "../src/renderer/lib/workspace-edit-transaction";
import {
  assertEditorBufferCleanForOverwrite,
  assertEditorBufferEditable,
  beginEditorBufferOperation,
  captureEditorBufferState,
  detachEditorBuffer,
  disposeEditorBuffer,
  editorBuffersForPath,
  getOrCreateEditorBuffer,
  isEditorBufferLocked,
  isEditorBufferStateCurrent,
  lockEditorBuffer,
  pathDerivedModelLanguage,
  rebindEditorBuffer,
  setLanguageDocumentAttacher,
  restoreDetachedEditorBuffer,
  restoreSuspendedEditorBuffer,
  subscribeEditorBufferModel,
  subscribeEditorBufferLock,
  suspendEditorBuffer,
} from "../src/renderer/lib/editor-buffers";
import { createDocumentSaveCoordinator } from "../src/renderer/lib/document-save-coordinator";
import { planWorkspaceEditModelLineages } from "../src/renderer/lib/workspace-edit-model-lineages";

function edit(): StagedWorkspaceEdit {
  return {
    transactionId: 7,
    authorization: "test-authorization",
    documents: [
      {
        workspaceId: 1,
        path: "open.ts",
        originalPath: "open.ts",
        originalText: "before",
        newText: "after",
        generation: 3,
        version: 5,
      },
    ],
    operations: [{ kind: "textDocument", document: 0 }],
  };
}

describe("workspace edit transaction coordinator", () => {
  test("commits, applies, and always finishes successful edits", async () => {
    const events: string[] = [];
    await applyWorkspaceEditTransaction(edit(), adapter(events));
    expect(events).toEqual([
      "preflight",
      "commit",
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
    staged.documents.push({
      ...staged.documents[0]!,
      path: "second.ts",
      originalPath: "second.ts",
    });
    staged.operations.push({ kind: "textDocument", document: 1 });
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

  test("revalidates each target in operation order after commit", async () => {
    const events: string[] = [];
    const stale = adapter(events);
    stale.preflight = (document) => ({
      document,
      validate() {
        events.push("validate");
        throw new Error("model changed during commit");
      },
      apply() {
        events.push("apply");
      },
      undo() {
        events.push("undo");
      },
    });
    await expect(applyWorkspaceEditTransaction(edit(), stale)).rejects.toThrow(
      "model changed during commit",
    );
    expect(events).toEqual(["commit", "validate", "rollback", "finish"]);
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
      "validate",
      "apply",
      "finish",
      "complete",
    ]);
  });

  test("committed cleanup recovery keeps models, unlocks them, and only retries finalize", async () => {
    const events: string[] = [];
    const current = adapter(events);
    current.preflight = (document) => ({
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
      complete() {
        events.push("complete");
      },
      release() {
        events.push("release");
      },
    });
    current.finish = async () => {
      events.push("finish");
      throw new Error("cleanup failed");
    };
    current.status = async (transactionId) => ({
      transactionId,
      phase: "committedCleanupRequired",
      retryRollback: false,
      canFinalize: true,
      requiresAcknowledgement: false,
    });
    current.reconcileCompletion = async () => {
      events.push("reconcile");
    };
    current.finalize = async (transactionId) => {
      events.push("finalize");
      return {
        transactionId,
        phase: "finishedCommitted",
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: false,
      };
    };

    await expect(applyWorkspaceEditTransaction(edit(), current)).rejects.toThrow(
      "cleanup still requires finalization",
    );
    expect(events).toEqual([
      "commit",
      "validate",
      "apply",
      "finish",
      "complete",
      "reconcile",
      "release",
    ]);
    expect(workspaceEditRecoveryActions()).toContainEqual({
      transactionId: 7,
      retryRollback: false,
      canFinalize: true,
    });
    expect(events).not.toContain("undo");
    expect(events).not.toContain("rollback");

    await finalizeWorkspaceEditRecovery(7);
    expect(events).toContain("finalize");
    expect(workspaceEditRecoveryIds()).not.toContain(7);
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

  test("discovers restart recoveries with fresh tokens and clears them after retry or finalize", async () => {
    const events: string[] = [];
    const authorizations: string[] = [];
    registerPersistedWorkspaceEditRecoveries(
      [
        {
          transactionId: 80,
          authorization: "fresh-retry-token",
          phase: "recoveryRequired",
          retryRollback: true,
          canFinalize: true,
          requiresAcknowledgement: false,
        },
        {
          transactionId: 81,
          authorization: "fresh-finalize-token",
          phase: "recoveryRequired",
          retryRollback: true,
          canFinalize: true,
          requiresAcknowledgement: false,
        },
        {
          transactionId: 82,
          authorization: "fresh-finished-token",
          phase: "finishedCommitted",
          retryRollback: false,
          canFinalize: false,
          requiresAcknowledgement: true,
        },
      ],
      (recovery) => {
        authorizations.push(recovery.authorization);
        let phase: "recoveryRequired" | "rolledBack" | "finishedRolledBack" | "finishedCommitted" =
          recovery.phase === "finishedCommitted" ? "finishedCommitted" : "recoveryRequired";
        return {
          validate() {},
          preflight() {
            return null;
          },
          async commitClosed() {},
          async rollbackClosed() {
            events.push(`rollback:${recovery.transactionId}`);
            phase = "rolledBack";
          },
          async finish() {
            events.push(`finish:${recovery.transactionId}`);
            phase = "finishedRolledBack";
          },
          async acknowledge() {
            events.push(`acknowledge:${recovery.transactionId}`);
          },
          async finalize() {
            events.push(`finalize:${recovery.transactionId}`);
            return {
              transactionId: recovery.transactionId,
              phase: "finishedCommitted",
              retryRollback: false,
              canFinalize: false,
              requiresAcknowledgement: false,
            };
          },
          async status() {
            return {
              transactionId: recovery.transactionId,
              phase,
              retryRollback: phase === "recoveryRequired",
              canFinalize: phase === "recoveryRequired",
              requiresAcknowledgement: phase.startsWith("finished"),
            };
          },
          reconcileCompletion() {
            events.push(`reconcile:${recovery.transactionId}`);
          },
        };
      },
    );

    expect(workspaceEditRecoveryIds()).toEqual(expect.arrayContaining([80, 81, 82]));
    expect(authorizations).toEqual([
      "fresh-retry-token",
      "fresh-finalize-token",
      "fresh-finished-token",
    ]);
    await retryWorkspaceEditRecovery(80);
    await finalizeWorkspaceEditRecovery(81);
    await retryWorkspaceEditRecovery(82);
    expect(events).toEqual([
      "rollback:80",
      "finish:80",
      "finalize:81",
      "reconcile:81",
      "reconcile:82",
      "acknowledge:82",
    ]);
    expect(workspaceEditRecoveryIds()).not.toContain(80);
    expect(workspaceEditRecoveryIds()).not.toContain(81);
    expect(workspaceEditRecoveryIds()).not.toContain(82);
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

  test("renamed models derive language from the destination URI", () => {
    expect(pathDerivedModelLanguage()).toBeUndefined();
  });

  test("an immediate edit and save after rename uses the replacement model", () => {
    const attached: string[] = [];
    setLanguageDocumentAttacher((_workspaceId, _tabId, path) => {
      attached.push(path);
      return { dispose() {} };
    });
    const source = fakeModel("dirty before rename");
    const replacement = fakeModel(source.getValue());
    const buffer = getOrCreateEditorBuffer(1, 91, "src/old.ts", "saved", () => source as never);
    let loadedEditorModel = buffer.model;
    const unsubscribe = subscribeEditorBufferModel(buffer, (model) => {
      loadedEditorModel = model;
    });

    rebindEditorBuffer(buffer, "src/new.ts", replacement as never);
    source.dispose();
    replacement.setValue("edited immediately");
    const saved = loadedEditorModel.getValue();

    expect(saved).toBe("edited immediately");
    expect(buffer.model).toBe(replacement as never);
    expect(buffer.savedContent).toBe("saved");
    expect(attached).toEqual(["src/old.ts", "src/new.ts"]);
    unsubscribe();
    disposeEditorBuffer(1, 91);
  });

  test("applies and recovers resource operations in documentChanges order", async () => {
    const events: string[] = [];
    const staged = edit();
    staged.documents[0]!.path = "renamed.ts";
    staged.documents[0]!.originalPath = "open.ts";
    staged.operations = [
      { kind: "renameFile", workspaceId: 1, oldPath: "open.ts", newPath: "renamed.ts" },
      { kind: "textDocument", document: 0 },
    ];
    const current = adapter(events);
    current.validate = () => events.push("revalidate");
    current.preflightResource = () => ({
      validate() {
        events.push("resource:validate");
      },
      apply() {
        events.push("resource:apply");
      },
      undo() {
        events.push("resource:undo");
      },
    });
    current.preflight = (document) => ({
      document,
      validate() {
        events.push("text:validate");
      },
      apply() {
        events.push("text:apply");
        throw new Error("text failed");
      },
      undo() {
        events.push("text:undo");
      },
    });

    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow("text failed");
    expect(events).toEqual([
      "commit",
      "resource:validate",
      "resource:apply",
      "text:validate",
      "text:apply",
      "text:undo",
      "resource:undo",
      "rollback",
      "finish",
    ]);
  });

  test("applies and undoes one coalesced model target around resources after a later failure", async () => {
    const events: string[] = [];
    const staged = edit();
    staged.documents[0] = {
      ...staged.documents[0]!,
      path: "renamed.ts",
      originalPath: "open.ts",
      originalText: "before",
      newText: "final",
    };
    staged.operations = [
      { kind: "renameFile", workspaceId: 1, oldPath: "open.ts", newPath: "renamed.ts" },
      { kind: "textDocument", document: 0 },
      { kind: "createFile", workspaceId: 1, path: "later.ts" },
    ];
    const current = adapter(events);
    let resource = 0;
    current.preflightResource = () => {
      const index = resource++;
      return {
        validate() {
          events.push(`resource:${index}:validate`);
        },
        apply() {
          events.push(`resource:${index}:apply`);
          if (index === 1) throw new Error("later resource failed");
        },
        undo() {
          events.push(`resource:${index}:undo`);
        },
      };
    };
    let modelTargets = 0;
    current.preflight = (document) => {
      modelTargets += 1;
      return {
        document,
        validate() {
          events.push("model:validate");
        },
        apply() {
          events.push("model:apply");
        },
        undo() {
          events.push("model:undo");
        },
      };
    };

    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "later resource failed",
    );

    expect(modelTargets).toBe(1);
    expect(events).toEqual([
      "commit",
      "resource:0:validate",
      "resource:0:apply",
      "model:validate",
      "model:apply",
      "resource:1:validate",
      "resource:1:apply",
      "resource:1:undo",
      "model:undo",
      "resource:0:undo",
      "rollback",
      "finish",
    ]);
  });

  test("text then delete retires the lineage without rejecting its virtual text", () => {
    const staged = lineageEdit([
      { kind: "textDocument", document: 0 },
      { kind: "deleteFile", workspaceId: 1, path: "old.ts", recursive: false },
    ]);
    expect(planWorkspaceEditModelLineages(staged, [lineageModel()])).toEqual([{
      ...lineageModel(),
      finalPath: null,
      finalContent: "after",
    }]);
  });

  test("text then overwrite-create retires the old model lineage directly", () => {
    const staged = lineageEdit([
      { kind: "textDocument", document: 0 },
      { kind: "createFile", workspaceId: 1, path: "old.ts" },
    ]);
    expect(planWorkspaceEditModelLineages(staged, [lineageModel()])).toEqual([{
      ...lineageModel(),
      finalPath: null,
      finalContent: "after",
    }]);
  });

  test("text then rename produces one destination model with final text", () => {
    const staged = lineageEdit([
      { kind: "textDocument", document: 0 },
      { kind: "renameFile", workspaceId: 1, oldPath: "old.ts", newPath: "new.ts" },
    ]);
    expect(planWorkspaceEditModelLineages(staged, [lineageModel()])).toEqual([{
      ...lineageModel(),
      finalPath: "new.ts",
      finalContent: "after",
    }]);
  });

  test("rename then text produces one destination model with final text", () => {
    const staged = lineageEdit([
      { kind: "renameFile", workspaceId: 1, oldPath: "old.ts", newPath: "new.ts" },
      { kind: "textDocument", document: 0 },
    ], "new.ts");
    expect(planWorkspaceEditModelLineages(staged, [lineageModel()])).toEqual([{
      ...lineageModel(),
      finalPath: "new.ts",
      finalContent: "after",
    }]);
  });

  test("multi rename-text chains apply one final lineage and roll back original state once", async () => {
    const staged = lineageEdit([
      { kind: "textDocument", document: 0 },
      { kind: "renameFile", workspaceId: 1, oldPath: "old.ts", newPath: "middle.ts" },
      { kind: "textDocument", document: 1 },
      { kind: "renameFile", workspaceId: 1, oldPath: "middle.ts", newPath: "final.ts" },
      { kind: "createFile", workspaceId: 1, path: "later.ts" },
    ]);
    staged.documents.push({
      ...staged.documents[0]!,
      path: "middle.ts",
      originalText: "after",
      newText: "final",
    });
    const [outcome] = planWorkspaceEditModelLineages(staged, [lineageModel()]);
    expect(outcome).toEqual({
      ...lineageModel(),
      finalPath: "final.ts",
      finalContent: "final",
    });

    const originalView = { line: 3 };
    const state = { path: "old.ts", content: "before", view: originalView };
    let applied = 0;
    let undone = 0;
    const current = adapter([]);
    current.preflightTargets = () => [{
      validate() {},
      apply() {
        applied += 1;
        state.path = outcome!.finalPath!;
        state.content = outcome!.finalContent;
        state.view = { line: 9 };
      },
      undo() {
        undone += 1;
        state.path = "old.ts";
        state.content = "before";
        state.view = originalView;
      },
    }, {
      validate() {},
      apply() {
        throw new Error("later target failed");
      },
      undo() {},
    }];

    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "later target failed",
    );
    expect({ ...state, applied, undone }).toEqual({
      path: "old.ts",
      content: "before",
      view: originalView,
      applied: 1,
      undone: 1,
    });
  });

  test("destructive validation uses the virtual operation state, not savedContent", () => {
    const authorized = lineageEdit([
      { kind: "textDocument", document: 0 },
      { kind: "deleteFile", workspaceId: 1, path: "old.ts", recursive: false },
    ]);
    expect(() => planWorkspaceEditModelLineages(authorized, [lineageModel()])).not.toThrow();

    const dirty = lineageModel();
    dirty.content = "user edit";
    const destructive = lineageEdit([
      { kind: "deleteFile", workspaceId: 1, path: "old.ts", recursive: false },
    ]);
    expect(() => planWorkspaceEditModelLineages(destructive, [dirty])).toThrow(
      "Cannot delete dirty open document old.ts",
    );
  });

  test("delete and rename buffers reject edits and saves until durable completion", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    for (const kind of ["deleteFile", "renameFile"] as const) {
      const tabId = kind === "deleteFile" ? 101 : 102;
      const buffer = getOrCreateEditorBuffer(
        1,
        tabId,
        "open.ts",
        "saved",
        () => fakeModel("saved") as never,
      );
      const lockEvents: boolean[] = [];
      const unsubscribe = subscribeEditorBufferLock(buffer, (locked) => lockEvents.push(locked));
      let continueCommit!: () => void;
      const commitBlocked = new Promise<void>((resolve) => {
        continueCommit = resolve;
      });
      const staged = edit();
      staged.documents = [];
      staged.operations = kind === "deleteFile"
        ? [{ kind, workspaceId: 1, path: "open.ts", recursive: false }]
        : [{ kind, workspaceId: 1, oldPath: "open.ts", newPath: "renamed.ts" }];
      const current = adapter([]);
      current.preflightResource = () => ({
        validate() {},
        apply() {},
        undo() {},
        release: lockEditorBuffer(buffer, staged.transactionId),
      });
      let phase: "staged" | "committed" | "finishedCommitted" = "staged";
      current.commitClosed = async () => {
        await commitBlocked;
        phase = "committed";
      };
      current.finish = async () => {
        phase = "finishedCommitted";
      };
      current.status = async (transactionId) => ({
        transactionId,
        phase,
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: false,
      });
      const applying = applyWorkspaceEditTransaction(staged, current);
      await Promise.resolve();

      expect(isEditorBufferLocked(buffer)).toBe(true);
      expect(() => assertEditorBufferEditable(buffer)).toThrow("locked");
      expect(lockEvents).toEqual([true]);
      continueCommit();
      await applying;
      expect(isEditorBufferLocked(buffer)).toBe(false);
      expect(() => assertEditorBufferEditable(buffer)).not.toThrow();
      expect(lockEvents).toEqual([true, false]);
      unsubscribe();
      disposeEditorBuffer(1, tabId);
    }
  });

  test("response-loss recovery retains the editor lock until status resolves", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const buffer = getOrCreateEditorBuffer(
      1,
      103,
      "open.ts",
      "before",
      () => fakeModel("before") as never,
    );
    let statusAvailable = false;
    const current = adapter([]);
    current.preflight = (document) => ({
      document,
      validate() {},
      apply() {},
      undo() {},
      release: lockEditorBuffer(buffer, 70),
    });
    current.commitClosed = async () => {
      throw new Error("connection lost");
    };
    current.status = async (transactionId) => {
      if (!statusAvailable) throw new Error("offline");
      return {
        transactionId,
        phase: "finishedRolledBack",
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: false,
      };
    };
    const staged = edit();
    staged.transactionId = 70;
    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "Recovery is incomplete",
    );
    expect(isEditorBufferLocked(buffer)).toBe(true);
    statusAvailable = true;
    await retryWorkspaceEditRecovery(70);
    expect(isEditorBufferLocked(buffer)).toBe(false);
    disposeEditorBuffer(1, 103);
  });

  test("ambiguous finish keeps applied models locked until committed status is confirmed", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const buffer = getOrCreateEditorBuffer(
      1,
      106,
      "open.ts",
      "before",
      () => fakeModel("before") as never,
    );
    const events: string[] = [];
    let statusAvailable = false;
    const current = adapter(events);
    current.preflight = (document) => ({
      document,
      validate() {},
      apply() {
        events.push("model:apply");
      },
      undo() {
        events.push("model:undo");
      },
      complete() {
        events.push("model:complete");
      },
      release: lockEditorBuffer(buffer, 71),
    });
    current.finish = async () => {
      events.push("finish");
    };
    current.acknowledge = async () => {
      events.push("acknowledge");
    };
    current.status = async (transactionId) => {
      if (!statusAvailable) throw new Error("status response lost");
      return {
        transactionId,
        phase: "finishedCommitted",
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: true,
      };
    };
    const staged = edit();
    staged.transactionId = 71;
    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "Recovery is incomplete",
    );
    expect(events).toContain("model:apply");
    expect(events).not.toContain("model:undo");
    expect(isEditorBufferLocked(buffer)).toBe(true);
    statusAvailable = true;
    await retryWorkspaceEditRecovery(71);
    expect(events).toContain("model:complete");
    expect(events.filter((event) => event === "finish")).toHaveLength(1);
    expect(events).toContain("acknowledge");
    expect(isEditorBufferLocked(buffer)).toBe(false);
    disposeEditorBuffer(1, 106);
  });

  test("restart rollback outcome undoes Monaco, acknowledges, and releases its lock", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const buffer = getOrCreateEditorBuffer(
      1,
      108,
      "open.ts",
      "before",
      () => fakeModel("before") as never,
    );
    const events: string[] = [];
    let statusAvailable = false;
    const current = adapter(events);
    current.preflight = (document) => ({
      document,
      validate() {},
      apply() {
        events.push("model:apply");
      },
      undo() {
        events.push("model:undo");
      },
      release: lockEditorBuffer(buffer, 73),
    });
    current.finish = async () => {
      events.push("finish");
      if (!statusAvailable) throw new Error("server restarted");
    };
    current.acknowledge = async () => {
      events.push("acknowledge");
    };
    current.status = async (transactionId) => {
      if (!statusAvailable) throw new Error("offline");
      return {
        transactionId,
        phase: "finishedRolledBack",
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: true,
      };
    };
    const staged = edit();
    staged.transactionId = 73;
    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "Recovery is incomplete",
    );
    expect(isEditorBufferLocked(buffer)).toBe(true);

    statusAvailable = true;
    await retryWorkspaceEditRecovery(73);
    expect(events).toContain("model:undo");
    expect(events.filter((event) => event === "finish")).toHaveLength(1);
    expect(events).toContain("acknowledge");
    expect(isEditorBufferLocked(buffer)).toBe(false);
    disposeEditorBuffer(1, 108);
  });

  test("unknown restart outcome performs safe reconciliation, warns, and unlocks", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const buffer = getOrCreateEditorBuffer(
      1,
      109,
      "open.ts",
      "before",
      () => fakeModel("before") as never,
    );
    const events: string[] = [];
    let reconnected = false;
    const current = adapter(events);
    current.preflight = (document) => ({
      document,
      validate() {},
      apply() {
        events.push("model:apply");
      },
      undo() {
        events.push("model:undo");
      },
      release: lockEditorBuffer(buffer, 74),
    });
    current.finish = async () => {
      events.push("finish");
      throw new Error("server restarted");
    };
    current.status = async () => {
      if (!reconnected) throw new Error("offline");
      throw Object.assign(new Error("workspace_edit.expired"), {
        code: "workspace_edit.expired",
      });
    };
    current.isUnknownTransaction = (error) =>
      Boolean(error && typeof error === "object" && "code" in error);
    current.reconcileUnknown = () => {
      events.push("full:reconcile");
    };
    const staged = edit();
    staged.transactionId = 74;
    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "Recovery is incomplete",
    );
    expect(isEditorBufferLocked(buffer)).toBe(true);

    reconnected = true;
    await retryWorkspaceEditRecovery(74);
    expect(events).not.toContain("model:undo");
    expect(events).toContain("full:reconcile");
    expect(isEditorBufferLocked(buffer)).toBe(false);
    expect(workspaceEditRecoveryWarnings().find(([id]) => id === 74)?.[1]).toContain(
      "editor locks were released",
    );
    dismissWorkspaceEditRecoveryWarning(74);
    disposeEditorBuffer(1, 109);
  });

  test("replacement buffers inherit unresolved transaction locks", () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const original = getOrCreateEditorBuffer(
      1,
      107,
      "old.ts",
      "old",
      () => fakeModel("old") as never,
    );
    const release = lockEditorBuffer(original, 72);
    const replacement = getOrCreateEditorBuffer(
      1,
      107,
      "new.ts",
      "new",
      () => fakeModel("new") as never,
    );
    expect(replacement).not.toBe(original);
    expect(isEditorBufferLocked(replacement)).toBe(true);
    expect(() => assertEditorBufferEditable(replacement)).toThrow("locked");
    release();
    expect(isEditorBufferLocked(replacement)).toBe(false);
    disposeEditorBuffer(1, 107);
  });

  test("overwrite rename keeps one destination buffer and restores both on rollback", () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const sourceModel = fakeModel("source");
    const destinationModel = fakeModel("destination");
    const source = getOrCreateEditorBuffer(
      1,
      104,
      "source.ts",
      "source",
      () => sourceModel as never,
    );
    const destination = getOrCreateEditorBuffer(
      1,
      105,
      "destination.ts",
      "destination",
      () => destinationModel as never,
    );
    destinationModel.setValue("dirty destination");
    expect(() => assertEditorBufferCleanForOverwrite(destination)).toThrow("dirty open document");
    destinationModel.setValue("destination");
    expect(() => assertEditorBufferCleanForOverwrite(destination)).not.toThrow();
    detachEditorBuffer(destination);
    destinationModel.dispose();
    const installed = fakeModel("source");
    rebindEditorBuffer(source, "destination.ts", installed as never);
    expect(editorBuffersForPath(1, "destination.ts")).toEqual([source]);

    rebindEditorBuffer(source, "source.ts", sourceModel as never);
    installed.dispose();
    const restoredDestination = fakeModel("destination");
    restoreDetachedEditorBuffer(
      destination,
      "destination.ts",
      restoredDestination as never,
    );
    expect(editorBuffersForPath(1, "source.ts")).toEqual([source]);
    expect(editorBuffersForPath(1, "destination.ts")).toEqual([destination]);
    disposeEditorBuffer(1, 104);
    disposeEditorBuffer(1, 105);
  });

  test("overwrite destination rejects late formatting, typing, and save results during commit", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const model = fakeModel("destination");
    const destination = getOrCreateEditorBuffer(
      1,
      110,
      "destination.ts",
      "destination",
      () => model as never,
    );
    const formatter = deferred<string>();
    const formattingOperation = beginEditorBufferOperation(destination, model as never);
    const formatting = formatter.promise.then((formatted) => {
      if (formattingOperation.isCurrent()) model.setValue(formatted);
    });
    const saveResult = deferred<void>();
    const saveCoordinator = createDocumentSaveCoordinator();
    const unsubscribe = subscribeEditorBufferLock(destination, (locked) => {
      if (locked) saveCoordinator.invalidate();
    });
    const save = saveCoordinator.begin(() => saveResult.promise);
    const saving = save.run(async (isCurrent) => {
      if (isCurrent()) destination.savedContent = model.getValue();
    });
    const commit = deferred<void>();
    const staged = edit();
    staged.transactionId = 75;
    staged.documents = [];
    staged.operations = [{
      kind: "renameFile",
      workspaceId: 1,
      oldPath: "source.ts",
      newPath: "destination.ts",
    }];
    const current = adapter([]);
    current.preflightResource = () => ({
      validate() {},
      apply() {},
      undo() {},
      release: lockEditorBuffer(destination, staged.transactionId),
    });
    const commitClosed = current.commitClosed;
    current.commitClosed = async (transactionId) => {
      await commit.promise;
      await commitClosed(transactionId);
    };
    const applying = applyWorkspaceEditTransaction(staged, current);
    await Promise.resolve();

    expect(isEditorBufferLocked(destination)).toBe(true);
    expect(() => assertEditorBufferEditable(destination)).toThrow("locked");
    expect(() => {
      assertEditorBufferEditable(destination);
      model.setValue("typed during commit");
    }).toThrow("locked");
    expect(() => {
      assertEditorBufferEditable(destination);
      destination.savedContent = model.getValue();
    }).toThrow("locked");
    formatter.resolve("formatted during commit");
    saveResult.resolve();
    await Promise.all([formatting, saving]);
    expect(model.getValue()).toBe("destination");
    expect(destination.savedContent).toBe("destination");

    commit.resolve();
    await applying;
    expect(isEditorBufferLocked(destination)).toBe(false);
    unsubscribe();
    disposeEditorBuffer(1, 110);
  });

  test("overwrite destination changes after preflight roll back without disposal", async () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const model = fakeModel("destination");
    const destination = getOrCreateEditorBuffer(
      1,
      111,
      "destination.ts",
      "destination",
      () => model as never,
    );
    const staged = edit();
    staged.transactionId = 76;
    staged.documents = [];
    staged.operations = [{
      kind: "renameFile",
      workspaceId: 1,
      oldPath: "source.ts",
      newPath: "destination.ts",
    }];
    const events: string[] = [];
    const current = adapter(events);
    current.preflightResource = () => {
      const state = captureEditorBufferState(destination);
      const release = lockEditorBuffer(destination, staged.transactionId);
      return {
        validate() {
          if (!isEditorBufferStateCurrent(state)) {
            throw new Error("Open rename destination changed before overwrite.");
          }
        },
        apply() {
          model.dispose();
        },
        undo() {},
        release,
      };
    };
    current.commitClosed = async () => {
      events.push("commit");
      model.setValue("external destination change");
    };

    await expect(applyWorkspaceEditTransaction(staged, current)).rejects.toThrow(
      "destination changed before overwrite",
    );
    expect(events).toEqual(["commit", "rollback", "finish"]);
    expect(model.isDisposed()).toBe(false);
    expect(model.getValue()).toBe("external destination change");
    expect(isEditorBufferLocked(destination)).toBe(false);
    disposeEditorBuffer(1, 111);
  });

  test("overwrite create removes old model lineage, restores on rollback, and disposes on commit", () => {
    setLanguageDocumentAttacher(() => ({ dispose() {} }));
    const oldModel = fakeModel("old content");
    const buffer = getOrCreateEditorBuffer(
      1,
      112,
      "a.ts",
      "old content",
      () => oldModel as never,
    );
    oldModel.setValue("dirty old content");
    expect(() => assertEditorBufferCleanForOverwrite(buffer)).toThrow("dirty open document");
    oldModel.setValue("old content");

    const suspended = suspendEditorBuffer(buffer);
    expect(oldModel.isDisposed()).toBe(true);
    expect(editorBuffersForPath(1, "a.ts")).toEqual([]);
    expect(editorBuffersForPath(1, "b.ts")).toEqual([]);
    const replacementContent = "new content";
    expect(replacementContent).not.toBe(suspended.content);

    const restoredModel = fakeModel(suspended.content);
    restoreSuspendedEditorBuffer(suspended, restoredModel as never);
    expect(editorBuffersForPath(1, "a.ts")).toEqual([buffer]);
    expect(buffer.model.getValue()).toBe("old content");
    expect(buffer.savedContent).toBe("old content");

    const committed = suspendEditorBuffer(buffer);
    expect(committed.model.isDisposed()).toBe(true);
    expect(editorBuffersForPath(1, "a.ts")).toEqual([]);
    expect(editorBuffersForPath(1, "b.ts")).toEqual([]);
    disposeEditorBuffer(1, 112);
  });
});

function fakeModel(initial: string) {
  let value = initial;
  let disposed = false;
  let version = 1;
  return {
    getValue: () => value,
    getVersionId: () => version,
    setValue(next: string) {
      value = next;
      version += 1;
    },
    isDisposed: () => disposed,
    dispose() {
      disposed = true;
    },
  };
}

function lineageEdit(
  operations: StagedWorkspaceEdit["operations"],
  path = "old.ts",
): StagedWorkspaceEdit {
  return {
    transactionId: 77,
    authorization: "lineage-test",
    documents: [{
      workspaceId: 1,
      path,
      originalPath: "old.ts",
      originalText: "before",
      newText: "after",
      generation: 2,
      version: 4,
    }],
    operations,
  };
}

function lineageModel() {
  return {
    workspaceId: 1,
    path: "old.ts",
    content: "before",
    savedContent: "before",
    value: "model",
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((currentResolve) => {
    resolve = currentResolve;
  });
  return { promise, resolve };
}

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
        complete() {
          events.push("complete");
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
    async finalize(transactionId) {
      phase = "finishedRolledBack";
      return {
        transactionId,
        phase,
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: false,
      };
    },
    async status(transactionId) {
      return {
        transactionId,
        phase,
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: false,
      };
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
      return {
        transactionId: 7,
        phase,
        retryRollback: false,
        canFinalize: false,
        requiresAcknowledgement: false,
      };
    },
    async status(transactionId) {
      return {
        transactionId,
        phase,
        retryRollback: phase === "recoveryRequired",
        canFinalize: phase === "recoveryRequired",
        requiresAcknowledgement: false,
      };
    },
  };
}
