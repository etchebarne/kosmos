import type { App } from "electron";
import fs from "node:fs";
import path from "node:path";

type DevelopmentApp = Pick<App, "getPath" | "isPackaged" | "setName" | "setPath">;

export function configureDevelopmentInstance(
  electronApp: DevelopmentApp,
  environment: NodeJS.ProcessEnv = process.env,
): string | null {
  if (electronApp.isPackaged) {
    return null;
  }

  if (environment.KOSMOS_PARENT_PID) {
    delete environment.KOSMOS_DATABASE;
    delete environment.KOSMOS_PARENT_PID;
    delete environment.KOSMOS_SOCKET;
  }

  const userDataPath = path.join(electronApp.getPath("appData"), "kosmos-development");
  fs.mkdirSync(userDataPath, { recursive: true });
  electronApp.setName("Kosmos Development");
  electronApp.setPath("userData", userDataPath);

  if (!environment.KOSMOS_DATABASE) {
    environment.KOSMOS_DATABASE = path.join(userDataPath, "state.sqlite3");
  }

  return userDataPath;
}
