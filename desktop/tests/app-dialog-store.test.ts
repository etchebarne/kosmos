import { beforeEach, expect, test } from "bun:test";

import { confirmDialog, showErrorDialog, useAppDialogStore } from "@/renderer/stores/app-dialog-store";

function queue() {
  return useAppDialogStore.getState().queue;
}

function resolveFront(confirmed: boolean) {
  const front = queue()[0];
  if (!front) throw new Error("no dialog queued");
  useAppDialogStore.getState().resolveDialog(front.id, confirmed);
}

beforeEach(() => {
  for (const dialog of queue()) {
    useAppDialogStore.getState().resolveDialog(dialog.id, false);
  }
});

test("concurrent requests queue in order and resolve independently", async () => {
  const first = confirmDialog({ title: "Delete a?", destructive: true });
  const error = showErrorDialog("fatal: bad ref", "Git command failed");
  const second = confirmDialog({ title: "Delete b?" });

  expect(queue().map((dialog) => dialog.title)).toEqual(["Delete a?", "Git command failed", "Delete b?"]);

  resolveFront(true);
  expect(await first).toBe(true);
  expect(queue()[0]).toMatchObject({ kind: "error", message: "fatal: bad ref" });

  resolveFront(false);
  await error;
  resolveFront(false);
  expect(await second).toBe(false);
  expect(queue()).toEqual([]);
});

test("error dialogs default their title", () => {
  void showErrorDialog("boom");
  expect(queue()[0]).toMatchObject({ kind: "error", title: "Something went wrong" });
});

test("resolving an unknown or already resolved dialog is ignored", async () => {
  const answer = confirmDialog({ title: "Proceed?" });
  const id = queue()[0]?.id ?? -1;

  useAppDialogStore.getState().resolveDialog(id, true);
  useAppDialogStore.getState().resolveDialog(id, false);
  useAppDialogStore.getState().resolveDialog(9999, false);

  expect(await answer).toBe(true);
  expect(queue()).toEqual([]);
});
