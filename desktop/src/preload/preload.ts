import { contextBridge, ipcRenderer } from "electron";

import type { KosmosApi, KosmosIpcRequest, KosmosIpcRequestResult } from "../shared/ipc";

class KosmosPreloadRequestError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(`${code}: ${message}`);
    this.name = "KosmosIpcRequestError";
  }
}

const kosmos: KosmosApi = {
  async request<T = unknown>(request: KosmosIpcRequest): Promise<T> {
    const response = (await ipcRenderer.invoke("kosmos:request", request)) as KosmosIpcRequestResult<T>;

    if (response.ok) {
      return response.result;
    }

    throw new KosmosPreloadRequestError(response.error.code, response.error.message);
  },
  getSocketPath(): Promise<string> {
    return ipcRenderer.invoke("kosmos:socketPath") as Promise<string>;
  },
  selectWorkspaceDirectory(): Promise<string | undefined> {
    return ipcRenderer.invoke("kosmos:selectWorkspaceDirectory") as Promise<string | undefined>;
  },
  minimizeWindow(): Promise<void> {
    return ipcRenderer.invoke("kosmos:window:minimize") as Promise<void>;
  },
  toggleMaximizeWindow(): Promise<void> {
    return ipcRenderer.invoke("kosmos:window:toggleMaximize") as Promise<void>;
  },
  closeWindow(): Promise<void> {
    return ipcRenderer.invoke("kosmos:window:close") as Promise<void>;
  },
  revealPath(path: string): Promise<void> {
    return ipcRenderer.invoke("kosmos:revealPath", path) as Promise<void>;
  },
};

contextBridge.exposeInMainWorld("kosmos", kosmos);
