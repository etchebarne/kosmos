import { describe, expect, test } from "bun:test";

import {
  createServerRecovery,
  type DegradedRecoveryState,
} from "@/main/server/recovery";

type Deferred<T> = {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: unknown): void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function harness() {
  let now = 0;
  let nextProcessToken = 100;
  let startServer = async () => nextProcessToken++;
  let reconnectClient = async () => {};
  let rendererReady = true;
  const events: string[] = [];
  const rendererGenerations: number[] = [];
  const degradedStates: DegradedRecoveryState[] = [];
  const recovery = createServerRecovery({
    now: () => now,
    disconnectClient: () => events.push("disconnect"),
    startServer: async () => {
      events.push("start");
      return startServer();
    },
    reconnectClient: async () => {
      events.push("reconnect");
      await reconnectClient();
    },
    requestRendererRestore: (generation) => {
      if (!rendererReady) {
        return false;
      }
      events.push(`restore:${generation}`);
      rendererGenerations.push(generation);
      return true;
    },
    onDegraded: (state) => degradedStates.push(state),
  });

  return {
    recovery,
    events,
    rendererGenerations,
    degradedStates,
    setNow(value: number) {
      now = value;
    },
    setStartServer(implementation: () => Promise<number>) {
      startServer = implementation;
    },
    setReconnectClient(implementation: () => Promise<void>) {
      reconnectClient = implementation;
    },
    setRendererReady(ready: boolean) {
      rendererReady = ready;
    },
  };
}

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("server recovery", () => {
  test("restarts, reconnects, and waits for the matching renderer generation", async () => {
    const { recovery, events } = harness();

    recovery.unexpectedExit(1, new Error("stopped"));
    await settle();

    expect(events).toEqual([
      "disconnect",
      "disconnect",
      "start",
      "reconnect",
      "restore:1",
    ]);
    expect(recovery.state).toMatchObject({ phase: "restoringRenderer", generation: 1 });
    expect(recovery.active).toBeTrue();
    recovery.rendererComplete(0);
    expect(recovery.active).toBeTrue();
    recovery.rendererComplete(1);
    expect(recovery.state).toEqual({ phase: "healthy", generation: 1 });
    expect(recovery.active).toBeFalse();
  });

  test("allows three automatic restarts before entering degraded state", async () => {
    const { recovery, events, degradedStates } = harness();

    for (let processToken = 1; processToken <= 3; processToken += 1) {
      recovery.unexpectedExit(processToken, new Error(`failure ${processToken}`));
      await settle();
      recovery.rendererComplete(recovery.generation);
    }
    recovery.unexpectedExit(4, new Error("fourth failure"));

    expect(events.filter((event) => event === "start")).toHaveLength(3);
    expect(recovery.state).toMatchObject({
      phase: "degraded",
      failure: { stage: "server", error: new Error("fourth failure") },
    });
    expect(degradedStates).toHaveLength(1);
    expect(recovery.active).toBeTrue();
  });

  test("expires automatic restarts outside the rolling window", async () => {
    const { recovery, events, setNow } = harness();

    for (let processToken = 1; processToken <= 3; processToken += 1) {
      recovery.unexpectedExit(processToken, new Error("stopped"));
      await settle();
      recovery.rendererComplete(recovery.generation);
    }
    setNow(60_001);
    recovery.unexpectedExit(4, new Error("later failure"));
    await settle();

    expect(events.filter((event) => event === "start")).toHaveLength(4);
    expect(recovery.state.phase).toBe("restoringRenderer");
  });

  test("enters degraded state when server start or reconnect fails", async () => {
    const startFailure = harness();
    startFailure.setStartServer(async () => {
      throw new Error("start failed");
    });
    startFailure.recovery.unexpectedExit(1, new Error("stopped"));
    await settle();
    expect(startFailure.recovery.state).toMatchObject({
      phase: "degraded",
      failure: { stage: "server", error: new Error("start failed") },
    });

    const reconnectFailure = harness();
    reconnectFailure.setReconnectClient(async () => {
      throw new Error("reconnect failed");
    });
    reconnectFailure.recovery.unexpectedExit(1, new Error("stopped"));
    await settle();
    expect(reconnectFailure.recovery.state).toMatchObject({
      phase: "degraded",
      failure: { stage: "server", error: new Error("reconnect failed") },
    });
  });

  test("ignores duplicate exit notifications for the same process", async () => {
    const { recovery, events, setStartServer } = harness();
    const pendingStart = deferred<number>();
    setStartServer(() => pendingStart.promise);

    recovery.unexpectedExit(1, new Error("stopped"));
    recovery.unexpectedExit(1, new Error("duplicate"));
    await settle();

    expect(events).toEqual(["disconnect", "disconnect", "start"]);
    expect(recovery.generation).toBe(1);
    pendingStart.resolve(2);
  });

  test("replacement exit invalidates a reconnect that is still pending", async () => {
    const { recovery, rendererGenerations, setReconnectClient } = harness();
    const firstReconnect = deferred<void>();
    let reconnects = 0;
    setReconnectClient(() => {
      reconnects += 1;
      return reconnects === 1 ? firstReconnect.promise : Promise.resolve();
    });

    recovery.unexpectedExit(1, new Error("first"));
    await settle();
    recovery.unexpectedExit(2, new Error("replacement"));
    await settle();
    firstReconnect.resolve();
    await settle();

    expect(rendererGenerations).toEqual([2]);
    expect(recovery.state).toMatchObject({ phase: "restoringRenderer", generation: 2 });
  });

  test("replacement exit invalidates pending renderer restoration", async () => {
    const { recovery, rendererGenerations } = harness();

    recovery.unexpectedExit(1, new Error("first"));
    await settle();
    recovery.unexpectedExit(2, new Error("replacement"));
    await settle();
    recovery.rendererComplete(1);

    expect(rendererGenerations).toEqual([1, 2]);
    expect(recovery.state).toMatchObject({ phase: "restoringRenderer", generation: 2 });
  });

  test("waits for renderer readiness before dispatching restoration", async () => {
    const { recovery, rendererGenerations, setRendererReady } = harness();
    setRendererReady(false);

    recovery.unexpectedExit(1, new Error("stopped"));
    await settle();
    expect(recovery.state.phase).toBe("restoringRenderer");
    expect(rendererGenerations).toEqual([]);

    setRendererReady(true);
    recovery.rendererAvailable();
    recovery.rendererAvailable();
    expect(rendererGenerations).toEqual([1]);
  });

  test("explicit retry restarts a failed sidecar and clears stale history", async () => {
    const { recovery, events, setStartServer } = harness();
    let starts = 0;
    setStartServer(async () => {
      starts += 1;
      if (starts === 1) throw new Error("start failed");
      return 101;
    });

    recovery.unexpectedExit(1, new Error("stopped"));
    await settle();
    const degraded = recovery.state as DegradedRecoveryState;
    expect(recovery.resolveDegraded(degraded.attemptToken, "retry")).toBe("retrying");
    await settle();

    expect(events.filter((event) => event === "start")).toHaveLength(2);
    expect(recovery.state).toMatchObject({ phase: "restoringRenderer", generation: 2 });
  });

  test("renderer failure retry redispatches restoration without reconnecting", async () => {
    const { recovery, events, rendererGenerations } = harness();

    recovery.unexpectedExit(1, new Error("stopped"));
    await settle();
    recovery.rendererComplete(1, "renderer restore failed");
    const degraded = recovery.state as DegradedRecoveryState;
    expect(degraded.failure.stage).toBe("renderer");
    expect(recovery.resolveDegraded(degraded.attemptToken, "retry")).toBe("retrying");

    expect(rendererGenerations).toEqual([1, 2]);
    expect(events.filter((event) => event === "start")).toHaveLength(1);
    expect(events.filter((event) => event === "reconnect")).toHaveLength(1);
    recovery.rendererComplete(1);
    expect(recovery.active).toBeTrue();
    recovery.rendererComplete(2);
    expect(recovery.active).toBeFalse();
    expect(recovery.resolveDegraded(degraded.attemptToken, "forceQuit")).toBe("ignored");
  });

  test("degraded choices keep open, force quit, and ignore stale responses", async () => {
    const { recovery, setStartServer } = harness();
    setStartServer(async () => {
      throw new Error("start failed");
    });
    recovery.unexpectedExit(1, new Error("stopped"));
    await settle();
    const degraded = recovery.state as DegradedRecoveryState;

    expect(recovery.resolveDegraded(degraded.attemptToken, "keepOpen")).toBe("keptOpen");
    expect(recovery.active).toBeTrue();
    expect(recovery.resolveDegraded(degraded.attemptToken, "forceQuit")).toBe("forceQuit");
    expect(recovery.resolveDegraded(degraded.attemptToken, "retry")).toBe("retrying");
    expect(recovery.resolveDegraded(degraded.attemptToken, "forceQuit")).toBe("forceQuit");
  });
});
