import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";

import { KosmosServerClient } from "../src/main/server/client";
import fixtures from "./fixtures/ipc/server-messages.json";

const cleanups: Array<() => Promise<void>> = [];

afterEach(async () => {
  await Promise.all(cleanups.splice(0).map((cleanup) => cleanup()));
});

describe("generated IPC contract validation", () => {
  test("delivers every valid notification and a validated action result", async () => {
    const received: string[] = [];
    const client = await clientForFrames([
      ...fixtures.notifications.map((message) => JSON.stringify(message)),
      JSON.stringify(fixtures.responses.success),
    ]);
    client.onNotification((message) => received.push(message.event));

    await expect(client.request("workspace", "list")).resolves.toEqual(fixtures.responses.success.result);
    expect(received).toEqual([
      "workspaceChanged",
      "languageServerDiagnosticsChanged",
      "languageServerDiagnosticsResync",
      "languageServerStatusChanged",
      "languageServerLogAvailable",
      "languageServerApplyEdit",
      "languageServerApplyEditCancelled",
    ]);
  });

  test("rejects malformed successful results before resolving the request", async () => {
    const client = await clientForFrames([response({ activeWorkspaceId: null })]);

    await expect(client.request("workspace", "list")).rejects.toThrow(
      "workspace.list",
    );
  });

  test("rejects malformed, unsafe, and unknown notifications before dispatch", async () => {
    for (const frame of [
      notification("workspaceChanged", { workspaceIds: "wrong" }),
      '{"type":"notification","event":"workspaceChanged","workspaceIds":[9007199254740992]}',
      notification("unknown", {}),
    ]) {
      const client = await clientForFrames([frame]);
      let delivered = false;
      client.onNotification(() => {
        delivered = true;
      });

      await expect(client.request("workspace", "list")).rejects.toThrow(
        "Invalid IPC notification",
      );
      expect(delivered).toBe(false);
    }
  });
});

async function clientForFrames(frames: string[]): Promise<KosmosServerClient> {
  const directory = await mkdtemp(path.join(os.tmpdir(), "kosmos-ipc-contract-"));
  const socketPath = path.join(directory, "server.sock");
  const sockets = new Set<net.Socket>();
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    socket.once("data", () => {
      socket.write(`${frames.join("\n")}\n`);
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
  cleanups.push(
    () => {
      for (const socket of sockets) {
        socket.destroy();
      }
      return new Promise<void>((resolve) => {
        server.close(() => resolve());
      }).then(() => rm(directory, { force: true, recursive: true }));
    },
  );
  return new KosmosServerClient(socketPath);
}

function notification(event: string, payload: Record<string, unknown>): string {
  return JSON.stringify({ type: "notification", event, ...payload });
}

function response(result: unknown): string {
  return JSON.stringify({ type: "response", id: 1, ok: true, result });
}
