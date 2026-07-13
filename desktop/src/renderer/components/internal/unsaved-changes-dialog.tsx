import { useEffect, useState } from "react";

import { Button } from "@/renderer/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/renderer/components/ui/dialog";
import { useWorkspaceStore } from "@/renderer/stores";
import type { CloseDocumentDecision } from "@/shared/ipc";

export function UnsavedChangesDialog() {
  const pending = useWorkspaceStore((state) => state.pendingClose);
  const resolvePendingClose = useWorkspaceStore((state) => state.resolvePendingClose);
  const [decisions, setDecisions] = useState<Record<string, CloseDocumentDecision>>({});

  useEffect(() => {
    setDecisions({});
  }, [pending?.closeId]);

  if (!pending) return null;

  const documentKey = (workspaceId: number, tabId: number) => `${workspaceId}:${tabId}`;
  const complete = pending.documents.every((document) =>
    Boolean(decisions[documentKey(document.workspaceId, document.tabId)]),
  );
  const chooseAll = (decision: CloseDocumentDecision) => {
    setDecisions(
      Object.fromEntries(
        pending.documents.map((document) => [documentKey(document.workspaceId, document.tabId), decision]),
      ),
    );
  };

  return (
    <Dialog open onOpenChange={(open) => !open && void resolvePendingClose("cancel")}>
      <DialogContent showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>Unsaved changes</DialogTitle>
          <DialogDescription>
            Choose what to do with every changed document before closing.
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-52 space-y-2 overflow-auto">
          {pending.documents.map((document) => {
            const key = documentKey(document.workspaceId, document.tabId);
            return (
              <div key={key} className="flex items-center justify-between gap-3 rounded border p-2">
                <span className="min-w-0 truncate font-mono text-xs">{document.path}</span>
                <div className="flex shrink-0 gap-1">
                  <Button
                    type="button"
                    size="sm"
                    variant={decisions[key] === "save" ? "default" : "outline"}
                    onClick={() => setDecisions((current) => ({ ...current, [key]: "save" }))}
                  >
                    Save
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant={decisions[key] === "discard" ? "destructive" : "outline"}
                    onClick={() => setDecisions((current) => ({ ...current, [key]: "discard" }))}
                  >
                    Discard
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => void resolvePendingClose("cancel")}>
            Cancel
          </Button>
          <Button type="button" variant="outline" onClick={() => chooseAll("discard")}>
            Discard all
          </Button>
          <Button type="button" variant="outline" onClick={() => chooseAll("save")}>
            Save all
          </Button>
          <Button type="button" disabled={!complete} onClick={() => void resolvePendingClose(decisions)}>
            Continue
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
