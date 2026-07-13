export type ShutdownAttemptOutcome = "completed" | "cancelled" | "failed" | "inProgress";

export function createShutdownAttempt(flushAndStop: () => Promise<void>) {
  let complete = false;
  let started = false;

  return {
    get complete(): boolean {
      return complete;
    },
    async attempt(resolveRenderer: () => Promise<boolean>): Promise<ShutdownAttemptOutcome> {
      if (complete) return "completed";
      if (started) return "inProgress";
      started = true;
      try {
        if (!(await resolveRenderer())) return "cancelled";
        await flushAndStop();
        complete = true;
        return "completed";
      } catch {
        return "failed";
      } finally {
        if (!complete) started = false;
      }
    },
  };
}
