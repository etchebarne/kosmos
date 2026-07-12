export type AsyncQueue = {
  run<T>(operation: () => Promise<T>): Promise<T>;
};

export function createAsyncQueue(): AsyncQueue {
  let tail = Promise.resolve();
  return {
    run(operation) {
      const result = tail.then(operation);
      tail = result.then(() => undefined, () => undefined);
      return result;
    },
  };
}
