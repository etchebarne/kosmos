import { Button } from "@/renderer/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/renderer/components/ui/dialog";
import { useWorkspaceStore, useWorkspaceTrustStore } from "@/renderer/stores";

export function WorkspaceTrustDialog() {
  const prompt = useWorkspaceTrustStore((state) => state.prompt);
  const trustWorkspace = useWorkspaceTrustStore((state) => state.trustWorkspace);
  const cancelWorkspaceTrust = useWorkspaceTrustStore((state) => state.cancelWorkspaceTrust);
  const closeWorkspaceTrust = useWorkspaceTrustStore((state) => state.closeWorkspaceTrust);
  const workspaceName = useWorkspaceStore((state) =>
    prompt
      ? state.snapshot?.workspaces.find((workspace) => workspace.id === prompt.workspaceId)?.name
      : undefined,
  );

  if (!prompt) {
    return null;
  }

  const workspaceLabel = workspaceName ?? `workspace ${prompt.workspaceId}`;

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) {
          closeWorkspaceTrust(prompt.workspaceId);
        }
      }}
    >
      <DialogContent showCloseButton={!prompt.isTrusting} aria-busy={prompt.isTrusting}>
        <DialogHeader>
          <DialogTitle>Trust {workspaceLabel}?</DialogTitle>
          <DialogDescription>
            Language tools can execute code controlled by this workspace with your permissions.
            Trusting it is remembered for this workspace.
          </DialogDescription>
        </DialogHeader>
        <p className="min-h-5 text-xs text-destructive" role={prompt.error ? "alert" : undefined}>
          {prompt.error}
        </p>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={prompt.isTrusting}
            onClick={() => cancelWorkspaceTrust(prompt.workspaceId)}
          >
            Cancel
          </Button>
          <Button
            type="button"
            disabled={prompt.isTrusting}
            onClick={() => void trustWorkspace(prompt.workspaceId)}
          >
            Trust workspace
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
