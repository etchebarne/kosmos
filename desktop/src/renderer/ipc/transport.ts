import type { KosmosIpcDomain, KosmosIpcParams } from "@/shared/ipc";

export function requestServer<T = unknown>(
  domain: KosmosIpcDomain,
  action: string,
  params?: KosmosIpcParams,
): Promise<T> {
  return kosmosApi().request<T>({ domain, action, params });
}

export function getSocketPath(): Promise<string> {
  return kosmosApi().getSocketPath();
}

export function selectWorkspaceDirectory(): Promise<string | undefined> {
  return kosmosApi().selectWorkspaceDirectory();
}

export function minimizeWindow(): Promise<void> {
  return kosmosApi().minimizeWindow();
}

export function toggleMaximizeWindow(): Promise<void> {
  return kosmosApi().toggleMaximizeWindow();
}

export function closeWindow(): Promise<void> {
  return kosmosApi().closeWindow();
}

export function revealPath(path: string): Promise<void> {
  return kosmosApi().revealPath(path);
}

function kosmosApi() {
  if (!window.kosmos) {
    throw new Error("Electron preload did not expose the Kosmos IPC API.");
  }

  return window.kosmos;
}
