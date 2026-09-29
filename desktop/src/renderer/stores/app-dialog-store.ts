import { create } from "zustand";

export type ConfirmDialogOptions = {
  title: string;
  description?: string;
  confirmLabel?: string;
  destructive?: boolean;
};

type AppDialogRequest =
  | { kind: "error"; title: string; message: string }
  | ({ kind: "confirm" } & ConfirmDialogOptions);

export type AppDialog = AppDialogRequest & { id: number };

type AppDialogStore = {
  queue: AppDialog[];
  showErrorDialog(message: string, title?: string): Promise<void>;
  confirmDialog(options: ConfirmDialogOptions): Promise<boolean>;
  resolveDialog(id: number, confirmed: boolean): void;
};

const DEFAULT_ERROR_TITLE = "Something went wrong";

export const useAppDialogStore = create<AppDialogStore>((set, get) => {
  const resolvers = new Map<number, (confirmed: boolean) => void>();
  let nextId = 1;

  function enqueue(request: AppDialogRequest): Promise<boolean> {
    const id = nextId++;
    const result = new Promise<boolean>((resolve) => resolvers.set(id, resolve));
    set((state) => ({ queue: [...state.queue, { ...request, id }] }));
    return result;
  }

  return {
    queue: [],
    async showErrorDialog(message, title = DEFAULT_ERROR_TITLE) {
      await enqueue({ kind: "error", title, message });
    },
    confirmDialog(options) {
      return enqueue({ kind: "confirm", ...options });
    },
    resolveDialog(id, confirmed) {
      const resolve = resolvers.get(id);
      if (!resolve) return;
      resolvers.delete(id);
      set({ queue: get().queue.filter((dialog) => dialog.id !== id) });
      resolve(confirmed);
    },
  };
});

export function showErrorDialog(message: string, title?: string): Promise<void> {
  return useAppDialogStore.getState().showErrorDialog(message, title);
}

export function confirmDialog(options: ConfirmDialogOptions): Promise<boolean> {
  return useAppDialogStore.getState().confirmDialog(options);
}
