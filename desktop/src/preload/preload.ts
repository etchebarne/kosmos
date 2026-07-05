import { contextBridge, ipcRenderer } from "electron";

import type { KosmosApi, KosmosIpcRequest } from "../shared/ipc";

const kosmos: KosmosApi = {
  request<T = unknown>(request: KosmosIpcRequest): Promise<T> {
    return ipcRenderer.invoke("kosmos:request", request) as Promise<T>;
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
};

contextBridge.exposeInMainWorld("kosmos", kosmos);
