import React, { useState, useEffect, createElement, type ComponentType } from "react";
import * as JsxRuntime from "react/jsx-runtime";
import { invoke } from "@tauri-apps/api/core";
import { registerTab, getTabDefinition } from "../tabs/registry";
import { usePluginStore } from "../store/plugin.store";
import { createPluginAPI } from "./api";
import type { Disposable, PluginModule, InstalledPlugin } from "./types";
import type { TabContentProps } from "../tabs/types";

/**
 * Globals used by the blob-URL React/JSX bridge modules to resolve bare
 * `react` / `react/jsx-runtime` specifiers from extension bundles.
 * See `buildReactBlobUrl` / `buildJsxBlobUrl` below.
 */
declare global {
  interface Window {
    __kr?: typeof React;
    __kj?: typeof JsxRuntime;
  }
}

/** Tracks disposables per plugin so we can clean up on deactivation. */
const pluginDisposables = new Map<string, Disposable[]>();

/** Cached plugin modules after dynamic import. */
const pluginModules = new Map<string, PluginModule>();

/**
 * Blob URLs that serve React to extensions.
 *
 * Extensions externalise `react` and `react/jsx-runtime` during their
 * esbuild step.  When we load extension code we rewrite those bare
 * specifiers to these blob URLs so the browser can resolve them — no
 * import-map, no shim files, no Vite plugin.
 *
 * The blob-URL modules read from two short-lived window properties that
 * are set in initPlugins() and exist solely as a bridge.
 */
let reactBlobUrl: string;
let jsxBlobUrl: string;

function buildReactBlobUrl(): string {
  const names = Object.keys(React).filter((k) => /^[a-zA-Z_$]/.test(k));
  const src = `const R=window.__kr;export default R;${names.map((n) => `export const ${n}=R.${n};`).join("")}`;
  return URL.createObjectURL(new Blob([src], { type: "text/javascript" }));
}

function buildJsxBlobUrl(): string {
  const src =
    "const J=window.__kj;" +
    "export const jsx=J.jsx,jsxs=J.jsxs,Fragment=J.Fragment,jsxDEV=J.jsxDEV||J.jsx;";
  return URL.createObjectURL(new Blob([src], { type: "text/javascript" }));
}

/**
 * Rewrite bare `react` / `react/jsx-runtime` specifiers in bundled
 * extension code so they point at our blob-URL modules.
 */
function rewriteReactImports(code: string): string {
  return code
    .replace(/from\s*["']react\/jsx-runtime["']/g, `from"${jsxBlobUrl}"`)
    .replace(/from\s*["']react\/jsx-dev-runtime["']/g, `from"${jsxBlobUrl}"`)
    .replace(/from\s*["']react["']/g, `from"${reactBlobUrl}"`);
}

/** Scan installed plugins, register stub tabs, and eagerly activate enabled ones. */
export async function initPlugins() {
  // Expose React for the blob-URL modules that plugins import from.
  window.__kr = React;
  window.__kj = JsxRuntime;
  reactBlobUrl = buildReactBlobUrl();
  jsxBlobUrl = buildJsxBlobUrl();

  const store = usePluginStore.getState();
  await store.init();

  const { plugins } = usePluginStore.getState();

  for (const [pluginId, plugin] of Object.entries(plugins)) {
    registerPluginStubs(pluginId, plugin);

    if (plugin.enabled) {
      activatePlugin(pluginId).catch((err) => {
        console.warn(`Failed to activate plugin "${pluginId}":`, err);
      });
    }
  }
}

/** Register stub tabs so they appear in the menu before the plugin is loaded. */
function registerPluginStubs(pluginId: string, plugin: InstalledPlugin) {
  const tabs = plugin.manifest.contributes?.tabs;
  if (!tabs) return;

  for (const tab of tabs) {
    registerTab({
      type: tab.type,
      title: tab.title,
      icon: tab.icon,
      defaultSize: tab.defaultSize,
      component: createLazyTabComponent(pluginId, tab.type),
    });
  }
}

/** Activates the plugin on first render, then swaps in the real component. */
function createLazyTabComponent(pluginId: string, tabType: string): ComponentType<TabContentProps> {
  const LazyPluginTab = (props: TabContentProps) => {
    const [Component, setComponent] = useState<ComponentType<TabContentProps> | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
      let cancelled = false;

      const resolve = () => {
        if (cancelled) return;
        const def = getTabDefinition(tabType);
        if (def && def.component !== LazyPluginTab) {
          setComponent(() => def.component);
        } else {
          setError("Plugin did not register its tab component");
        }
      };

      const plugin = usePluginStore.getState().plugins[pluginId];
      if (plugin?.activated) {
        resolve();
      } else {
        activatePlugin(pluginId)
          .then(resolve)
          .catch((err: Error) => {
            if (!cancelled) setError(err.message);
          });
      }

      return () => {
        cancelled = true;
      };
    }, []);

    if (error) {
      return createElement(
        "div",
        {
          className:
            "flex items-center justify-center h-full text-xs text-[var(--color-status-red)]",
        },
        `Plugin error: ${error}`,
      );
    }

    if (!Component) {
      return createElement(
        "div",
        {
          className:
            "flex items-center justify-center h-full text-xs text-[var(--color-text-secondary)] animate-pulse",
        },
        "Loading plugin…",
      );
    }

    return createElement(Component, props);
  };

  LazyPluginTab.displayName = `LazyPlugin(${pluginId}/${tabType})`;
  return LazyPluginTab;
}

/** Dynamic-import the plugin entry and call activate(). No-op if already active. */
async function activatePlugin(pluginId: string): Promise<void> {
  const store = usePluginStore.getState();
  const plugin = store.plugins[pluginId];
  if (!plugin || plugin.activated) return;

  // Flip the flag early so a concurrent resolve() doesn't double-activate.
  store.markActivated(pluginId);

  try {
    // file:// imports are blocked by the webview origin policy; load via a same-origin
    // blob URL, rewriting bare "react" specifiers to a shared React blob.
    const entryPath = `${plugin.path}/${plugin.manifest.main}`.split("\\").join("/");
    const raw: string = await invoke("read_file", { path: entryPath });
    const blob = new Blob([rewriteReactImports(raw)], { type: "text/javascript" });
    const blobUrl = URL.createObjectURL(blob);

    let mod: PluginModule;
    try {
      mod = await import(/* @vite-ignore */ blobUrl);
    } finally {
      URL.revokeObjectURL(blobUrl);
    }
    pluginModules.set(pluginId, mod);

    const { api, disposables } = createPluginAPI(pluginId);
    pluginDisposables.set(pluginId, disposables);

    await mod.activate(api);
  } catch (err) {
    const plugins = { ...usePluginStore.getState().plugins };
    if (plugins[pluginId]) {
      plugins[pluginId] = { ...plugins[pluginId], activated: false };
      usePluginStore.setState({ plugins });
    }
    throw err;
  }
}

/**
 * Deactivate a plugin — call its deactivate() and dispose all registered resources.
 */
export async function deactivatePlugin(pluginId: string): Promise<void> {
  const mod = pluginModules.get(pluginId);
  if (mod?.deactivate) {
    await mod.deactivate();
  }
  pluginModules.delete(pluginId);

  const disposables = pluginDisposables.get(pluginId);
  if (disposables) {
    for (const d of disposables) {
      d.dispose();
    }
    pluginDisposables.delete(pluginId);
  }

  const store = usePluginStore.getState();
  const plugin = store.plugins[pluginId];
  if (plugin) {
    const plugins = { ...store.plugins };
    plugins[pluginId] = { ...plugin, activated: false };
    usePluginStore.setState({ plugins });
  }
}
