export type RecoveryFailure = {
  stage: "server" | "renderer";
  error: Error;
};

export type RecoveryState =
  | { phase: "healthy"; generation: number }
  | { phase: "restarting"; attemptToken: number; generation: number }
  | { phase: "restoringRenderer"; attemptToken: number; generation: number }
  | {
      phase: "degraded";
      attemptToken: number;
      generation: number;
      failure: RecoveryFailure;
    };

export type DegradedRecoveryState = Extract<RecoveryState, { phase: "degraded" }>;
export type RecoveryChoice = "retry" | "keepOpen" | "forceQuit";
export type RecoveryChoiceOutcome = "ignored" | "retrying" | "keptOpen" | "forceQuit";

type RecoveryDependencies = {
  now(): number;
  disconnectClient(): void;
  startServer(): Promise<number>;
  reconnectClient(): Promise<void>;
  requestRendererRestore(generation: number): boolean;
  onDegraded(state: DegradedRecoveryState): void;
};

export type ServerRecoveryController = ReturnType<typeof createServerRecovery>;

const MAX_AUTOMATIC_RESTARTS = 3;
const AUTOMATIC_RESTART_WINDOW_MS = 60_000;

export function createServerRecovery(dependencies: RecoveryDependencies) {
  let state: RecoveryState = { phase: "healthy", generation: 0 };
  let nextAttemptToken = 1;
  let lastExitedProcessToken = 0;
  let activeServerRecovery = Promise.resolve();
  let dispatchedRendererGeneration: number | undefined;
  const automaticRestartTimes: number[] = [];

  function isCurrent(attemptToken: number): boolean {
    return state.phase !== "healthy" && state.attemptToken === attemptToken;
  }

  function nextAttempt(phase: "restarting" | "restoringRenderer"): {
    attemptToken: number;
    generation: number;
  } {
    const attemptToken = nextAttemptToken++;
    const generation = state.generation + 1;
    dispatchedRendererGeneration = undefined;
    state = { phase, attemptToken, generation };
    return { attemptToken, generation };
  }

  function degrade(attemptToken: number, failure: RecoveryFailure): void {
    if (!isCurrent(attemptToken)) {
      return;
    }
    state = {
      phase: "degraded",
      attemptToken,
      generation: state.generation,
      failure,
    };
    dependencies.onDegraded(state);
  }

  function requestRendererRestore(attemptToken: number, generation: number): void {
    if (!isCurrent(attemptToken) || dispatchedRendererGeneration === generation) {
      return;
    }
    state = { phase: "restoringRenderer", attemptToken, generation };
    try {
      if (dependencies.requestRendererRestore(generation)) {
        dispatchedRendererGeneration = generation;
      }
    } catch (caughtError: unknown) {
      degrade(attemptToken, { stage: "renderer", error: asError(caughtError) });
    }
  }

  async function recoverServer(attemptToken: number, generation: number): Promise<void> {
    try {
      dependencies.disconnectClient();
      const processToken = await dependencies.startServer();
      if (!isCurrent(attemptToken)) {
        return;
      }
      if (processToken <= lastExitedProcessToken) {
        throw new Error("Replacement server exited before recovery completed");
      }
      await dependencies.reconnectClient();
      requestRendererRestore(attemptToken, generation);
    } catch (caughtError: unknown) {
      degrade(attemptToken, { stage: "server", error: asError(caughtError) });
    }
  }

  function beginServerRecovery(): void {
    const { attemptToken, generation } = nextAttempt("restarting");
    try {
      dependencies.disconnectClient();
    } catch (caughtError: unknown) {
      degrade(attemptToken, { stage: "server", error: asError(caughtError) });
      return;
    }
    activeServerRecovery = activeServerRecovery.then(() => {
      if (isCurrent(attemptToken)) {
        return recoverServer(attemptToken, generation);
      }
    });
  }

  function beginRendererRecovery(): void {
    const { attemptToken, generation } = nextAttempt("restoringRenderer");
    requestRendererRestore(attemptToken, generation);
  }

  function pruneAutomaticRestartHistory(now: number): void {
    while (
      automaticRestartTimes.length > 0 &&
      now - automaticRestartTimes[0]! > AUTOMATIC_RESTART_WINDOW_MS
    ) {
      automaticRestartTimes.shift();
    }
  }

  return {
    get state(): RecoveryState {
      return state;
    },
    get active(): boolean {
      return state.phase !== "healthy";
    },
    get restoringRenderer(): boolean {
      return state.phase === "restoringRenderer";
    },
    get generation(): number {
      return state.generation;
    },
    unexpectedExit(processToken: number, exitError: Error): void {
      if (!Number.isSafeInteger(processToken) || processToken <= lastExitedProcessToken) {
        return;
      }
      lastExitedProcessToken = processToken;

      const now = dependencies.now();
      pruneAutomaticRestartHistory(now);
      if (automaticRestartTimes.length >= MAX_AUTOMATIC_RESTARTS) {
        const { attemptToken } = nextAttempt("restarting");
        try {
          dependencies.disconnectClient();
        } catch {
          // The original sidecar failure remains the actionable recovery error.
        }
        degrade(attemptToken, { stage: "server", error: exitError });
        return;
      }

      automaticRestartTimes.push(now);
      beginServerRecovery();
    },
    rendererComplete(generation: number, error?: string): void {
      if (state.phase !== "restoringRenderer" || state.generation !== generation) {
        return;
      }
      if (error) {
        degrade(state.attemptToken, { stage: "renderer", error: new Error(error) });
        return;
      }
      state = { phase: "healthy", generation };
    },
    rendererAvailable(): void {
      if (state.phase === "restoringRenderer") {
        requestRendererRestore(state.attemptToken, state.generation);
      }
    },
    resolveDegraded(attemptToken: number, choice: RecoveryChoice): RecoveryChoiceOutcome {
      if (state.phase !== "degraded" || state.attemptToken !== attemptToken) {
        if (choice === "forceQuit" && state.phase !== "healthy") {
          return "forceQuit";
        }
        return "ignored";
      }
      if (choice === "keepOpen") {
        return "keptOpen";
      }
      if (choice === "forceQuit") {
        return "forceQuit";
      }

      automaticRestartTimes.length = 0;
      if (state.failure.stage === "renderer") {
        beginRendererRecovery();
      } else {
        beginServerRecovery();
      }
      return "retrying";
    },
  };
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
