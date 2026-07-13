import { afterEach, describe, expect, test } from "bun:test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { configureDevelopmentInstance } from "@/main/development-instance";

const testDirectories: string[] = [];

afterEach(() => {
  for (const directory of testDirectories.splice(0)) {
    fs.rmSync(directory, { force: true, recursive: true });
  }
});

describe("development instance", () => {
  test("isolates unpackaged user data and server state", () => {
    const appData = temporaryDirectory("isolated");
    const calls = { name: "", userData: "" };
    const environment: NodeJS.ProcessEnv = {};
    const userData = configureDevelopmentInstance(
      {
        isPackaged: false,
        getPath: () => appData,
        setName: (name) => {
          calls.name = name;
        },
        setPath: (name, value) => {
          if (name === "userData") calls.userData = value;
        },
      },
      environment,
    );

    const expectedUserData = path.join(appData, "kosmos-development");
    expect(userData).toBe(expectedUserData);
    expect(calls).toEqual({ name: "Kosmos Development", userData: expectedUserData });
    expect(environment.KOSMOS_DATABASE).toBe(path.join(expectedUserData, "state.sqlite3"));
    expect(fs.statSync(expectedUserData).isDirectory()).toBe(true);
  });

  test("does not change packaged instances or explicit database overrides", () => {
    const appData = temporaryDirectory("packaged");
    const environment: NodeJS.ProcessEnv = { KOSMOS_DATABASE: "/custom/state.sqlite3" };
    const calls: string[] = [];
    const app = {
      isPackaged: true,
      getPath: () => appData,
      setName: () => calls.push("name"),
      setPath: () => calls.push("path"),
    };

    expect(configureDevelopmentInstance(app, environment)).toBeNull();
    expect(calls).toEqual([]);
    expect(environment.KOSMOS_DATABASE).toBe("/custom/state.sqlite3");

    app.isPackaged = false;
    configureDevelopmentInstance(app, environment);
    expect(environment.KOSMOS_DATABASE).toBe("/custom/state.sqlite3");
  });

  test("drops sidecar variables inherited from a parent Kosmos terminal", () => {
    const appData = temporaryDirectory("inherited");
    const environment: NodeJS.ProcessEnv = {
      KOSMOS_DATABASE: "/production/state.sqlite3",
      KOSMOS_PARENT_PID: "1234",
      KOSMOS_SOCKET: "/production/server.sock",
    };

    configureDevelopmentInstance(
      {
        isPackaged: false,
        getPath: () => appData,
        setName: () => undefined,
        setPath: () => undefined,
      },
      environment,
    );

    expect(environment.KOSMOS_PARENT_PID).toBeUndefined();
    expect(environment.KOSMOS_SOCKET).toBeUndefined();
    expect(environment.KOSMOS_DATABASE).toBe(
      path.join(appData, "kosmos-development", "state.sqlite3"),
    );
  });
});

function temporaryDirectory(name: string): string {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), `kosmos-development-${name}-`));
  testDirectories.push(directory);
  return directory;
}
