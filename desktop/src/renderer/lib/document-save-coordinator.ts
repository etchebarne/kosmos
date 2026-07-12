import { createAsyncQueue } from "./async-queue";

export type CoordinatedDocumentSave = {
  isCurrent(): boolean;
  run(operation: (isCurrent: () => boolean) => Promise<void>): Promise<void>;
};

export type DocumentSaveCoordinator = {
  begin(safetySnapshot: () => Promise<unknown>): CoordinatedDocumentSave;
  invalidate(): void;
};

export function createDocumentSaveCoordinator(): DocumentSaveCoordinator {
  const queue = createAsyncQueue();
  let generation = 0;
  return {
    begin(safetySnapshot) {
      const saveGeneration = ++generation;
      const snapshot = safetySnapshot();
      const isCurrent = () => saveGeneration === generation;
      return {
        isCurrent,
        run(operation) {
          return queue.run(async () => {
            await snapshot;
            if (isCurrent()) {
              await operation(isCurrent);
            }
          });
        },
      };
    },
    invalidate() {
      generation += 1;
    },
  };
}
