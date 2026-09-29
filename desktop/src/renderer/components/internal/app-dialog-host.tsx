import { Button } from "@/renderer/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/renderer/components/ui/dialog";
import { useAppDialogStore } from "@/renderer/stores";
import type { AppDialog } from "@/renderer/stores/app-dialog-store";

type DialogBodyProps<Kind extends AppDialog["kind"]> = {
  dialog: Extract<AppDialog, { kind: Kind }>;
  resolve(confirmed: boolean): void;
};

export function AppDialogHost() {
  const dialog = useAppDialogStore((state) => state.queue[0]);
  const resolveDialog = useAppDialogStore((state) => state.resolveDialog);

  if (!dialog) return null;

  const resolve = (confirmed: boolean) => resolveDialog(dialog.id, confirmed);

  return (
    <Dialog key={dialog.id} open onOpenChange={(open) => !open && resolve(false)}>
      {dialog.kind === "error" ? (
        <ErrorDialogBody dialog={dialog} resolve={resolve} />
      ) : (
        <ConfirmDialogBody dialog={dialog} resolve={resolve} />
      )}
    </Dialog>
  );
}

function ErrorDialogBody({ dialog, resolve }: DialogBodyProps<"error">) {
  return (
    <DialogContent className="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{dialog.title}</DialogTitle>
        <DialogDescription
          role="alert"
          className="max-h-64 overflow-auto rounded-md border bg-muted/50 p-2 font-mono text-xs break-words whitespace-pre-wrap text-foreground select-text"
        >
          {dialog.message}
        </DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button type="button" autoFocus onClick={() => resolve(true)}>
          OK
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}

function ConfirmDialogBody({ dialog, resolve }: DialogBodyProps<"confirm">) {
  return (
    <DialogContent showCloseButton={false}>
      <DialogHeader>
        <DialogTitle>{dialog.title}</DialogTitle>
        {dialog.description ? <DialogDescription>{dialog.description}</DialogDescription> : null}
      </DialogHeader>
      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          autoFocus={dialog.destructive}
          onClick={() => resolve(false)}
        >
          Cancel
        </Button>
        <Button
          type="button"
          autoFocus={!dialog.destructive}
          variant={dialog.destructive ? "destructive" : "default"}
          onClick={() => resolve(true)}
        >
          {dialog.confirmLabel ?? "Confirm"}
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}
