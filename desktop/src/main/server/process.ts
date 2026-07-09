import { app } from "electron";
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";

const SERVER_START_TIMEOUT_MS = 5_000;
const SERVER_READY_POLL_MS = 50;
const SOCKET_CONNECT_TIMEOUT_MS = 100;

export class KosmosServerProcess {
  private child: ChildProcess | undefined;

  constructor(private readonly socketPath: string) {}

  async start(): Promise<void> {
    if (await canConnectToSocket(this.socketPath)) {
      return;
    }

    const child = this.spawn();
    await waitForServerSocket(child, this.socketPath);
  }

  stop(): void {
    if (!this.child || this.child.killed) {
      return;
    }

    this.child.kill();
    this.child = undefined;
  }

  private spawn(): ChildProcess {
    const serverPath = getServerBinaryPath();
    if (!fs.existsSync(serverPath)) {
      throw new Error(`Kosmos server binary was not found at ${serverPath}`);
    }

    const child = spawn(serverPath, [], {
      env: {
        ...process.env,
        KOSMOS_SOCKET: this.socketPath,
      },
      stdio: "ignore",
    });

    this.child = child;
    child.once("exit", () => {
      if (this.child === child) {
        this.child = undefined;
      }
    });

    return child;
  }
}

function getServerBinaryPath(): string {
  if (app.isPackaged) {
    return path.join(process.resourcesPath, "bin", "kosmos-server");
  }

  return path.resolve(app.getAppPath(), "..", "target", "debug", "kosmos-server");
}

async function waitForServerSocket(child: ChildProcess, socketPath: string): Promise<void> {
  const deadline = Date.now() + SERVER_START_TIMEOUT_MS;
  let startError: Error | undefined;
  const onError = (error: Error) => {
    startError = error;
  };

  child.once("error", onError);

  try {
    while (Date.now() < deadline) {
      if (startError) {
        throw startError;
      }

      if (child.exitCode !== null || child.signalCode !== null) {
        throw new Error("Kosmos server exited before opening its IPC socket");
      }

      if (await canConnectToSocket(socketPath)) {
        return;
      }

      await sleep(SERVER_READY_POLL_MS);
    }
  } finally {
    child.off("error", onError);
  }

  throw new Error(`Kosmos server did not open its IPC socket at ${socketPath}`);
}

function canConnectToSocket(socketPath: string): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = net.createConnection(socketPath);
    let settled = false;

    function finish(isReady: boolean): void {
      if (settled) {
        return;
      }

      settled = true;
      socket.removeAllListeners();
      socket.destroy();
      resolve(isReady);
    }

    socket.setTimeout(SOCKET_CONNECT_TIMEOUT_MS);
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
    socket.once("timeout", () => finish(false));
  });
}

function sleep(durationMs: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, durationMs));
}
