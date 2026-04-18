import { load, type Store } from "@tauri-apps/plugin-store";

const stores = new Map<string, Store>();

export async function getTauriStore(filename: string): Promise<Store> {
  let store = stores.get(filename);
  if (!store) {
    store = await load(filename, { defaults: {}, autoSave: true });
    stores.set(filename, store);
  }
  return store;
}
