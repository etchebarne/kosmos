import type { KosmosApi } from "../shared/ipc";

declare global {
  interface Window {
    kosmos: KosmosApi;
  }
}

export {};
