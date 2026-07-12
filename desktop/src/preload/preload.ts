import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";

import type {
  KosmosApi,
  KosmosIpcRequest,
  KosmosIpcRequestResult,
  KosmosServerNotification,
  SettingsSnapshot,
  WorkspaceId,
} from "../shared/ipc";
import { reconstructIpcRequestResult } from "./request-result";

const kosmos: KosmosApi = {
  async request<T = unknown>(request: KosmosIpcRequest): Promise<KosmosIpcRequestResult<T>> {
    const response = (await ipcRenderer.invoke("kosmos:request", request)) as KosmosIpcRequestResult<T>;
    return reconstructIpcRequestResult<T>(response);
  },
  cancelRequest(requestKey: string): void {
    ipcRenderer.send("kosmos:cancelRequest", requestKey);
  },
  acknowledgeServerApplyEdit(
    id: number,
    token: string,
    applied: boolean,
    failureReason?: string,
  ): void {
    ipcRenderer.send("kosmos:serverApplyEditAck", { id, token, applied, failureReason });
  },
  pendingServerApplyEdits() {
    return ipcRenderer.invoke("kosmos:pendingServerApplyEdits") as ReturnType<
      KosmosApi["pendingServerApplyEdits"]
    >;
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
  bootstrapSettings(): Promise<SettingsSnapshot> {
    return ipcRenderer.invoke("kosmos:bootstrapSettings") as Promise<SettingsSnapshot>;
  },
  revealPath(path: string): Promise<void> {
    return ipcRenderer.invoke("kosmos:revealPath", path) as Promise<void>;
  },
  onFlushState(callback: () => Promise<void>): () => void {
    const listener = () => {
      void callback().finally(() => ipcRenderer.send("kosmos:rendererStateFlushed"));
    };

    ipcRenderer.on("kosmos:flushState", listener);
    return () => ipcRenderer.off("kosmos:flushState", listener);
  },
  onShutdownRequest(callback: () => Promise<boolean>): () => void {
    const listener = () => {
      void callback()
        .then((approved) => ipcRenderer.send("kosmos:shutdownResolved", { approved }))
        .catch((error: unknown) =>
          ipcRenderer.send("kosmos:shutdownResolved", {
            approved: false,
            error: error instanceof Error ? error.message : String(error),
          }),
        );
    };
    ipcRenderer.on("kosmos:prepareShutdown", listener);
    return () => ipcRenderer.off("kosmos:prepareShutdown", listener);
  },
  onSettingsSnapshot(callback: (snapshot: SettingsSnapshot) => void): () => void {
    const listener = (_event: IpcRendererEvent, snapshot: SettingsSnapshot) => {
      callback(snapshot);
    };

    ipcRenderer.on("kosmos:settingsSnapshot", listener);
    return () => ipcRenderer.off("kosmos:settingsSnapshot", listener);
  },
  onWorkspaceChanged(callback: (workspaceIds: WorkspaceId[]) => void): () => void {
    const listener = (_event: IpcRendererEvent, workspaceIds: WorkspaceId[]) => {
      callback(workspaceIds);
    };

    ipcRenderer.on("kosmos:workspaceChanged", listener);
    return () => ipcRenderer.off("kosmos:workspaceChanged", listener);
  },
  onServerNotification(callback: (notification: KosmosServerNotification) => void): () => void {
    const listener = (_event: IpcRendererEvent, notification: KosmosServerNotification) => {
      callback(notification);
    };
    ipcRenderer.on("kosmos:serverNotification", listener);
    return () => ipcRenderer.off("kosmos:serverNotification", listener);
  },
  onServerReconnected(callback: () => void): () => void {
    const listener = () => callback();
    ipcRenderer.on("kosmos:serverReconnected", listener);
    return () => ipcRenderer.off("kosmos:serverReconnected", listener);
  },
};

contextBridge.exposeInMainWorld("kosmos", kosmos);
