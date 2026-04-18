import { useEffect, useState } from "react";
import {
  PuzzlePiece,
  DownloadSimple,
  Trash,
  ArrowClockwise,
  CircleNotch,
  Globe,
  ToggleLeft,
  ToggleRight,
} from "@phosphor-icons/react";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { SectionTitle } from "../../components/shared/SectionTitle";
import { usePluginStore } from "../../store/plugin.store";
import { useToastStore } from "../../store/toast.store";
import { deactivatePlugin } from "../../plugins/host";
import { useShallow } from "zustand/react/shallow";
import type { TabContentProps } from "../types";
import { registryEntryId, type InstalledPlugin, type RegistryEntry } from "../../plugins/types";

function InstalledPluginCard({ plugin }: { plugin: InstalledPlugin }) {
  const { uninstall, setEnabled } = usePluginStore(
    useShallow((s) => ({ uninstall: s.uninstall, setEnabled: s.setEnabled })),
  );
  const addToast = useToastStore((s) => s.addToast);
  const [removing, setRemoving] = useState(false);

  const handleUninstall = async () => {
    setRemoving(true);
    try {
      if (plugin.activated) {
        await deactivatePlugin(plugin.pluginId);
      }
      await uninstall(plugin.pluginId);
      addToast({ message: `Uninstalled "${plugin.manifest.name}"`, type: "success" });
    } catch {
      addToast({ message: `Failed to uninstall "${plugin.manifest.name}"`, type: "error" });
    } finally {
      setRemoving(false);
    }
  };

  const handleToggle = async () => {
    if (plugin.enabled && plugin.activated) {
      await deactivatePlugin(plugin.pluginId);
    }
    setEnabled(plugin.pluginId, !plugin.enabled);
    addToast({
      message: `${plugin.enabled ? "Disabled" : "Enabled"} "${plugin.manifest.name}" — restart to apply`,
      type: "info",
    });
  };

  return (
    <div className="flex items-start gap-3 px-4 py-3 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] shadow-[2px_2px_0_rgba(0,0,0,0.15)] rounded-md">
      <PuzzlePiece
        size={20}
        weight="duotone"
        className="shrink-0 mt-0.5 text-[var(--color-accent-blue)]"
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-xs font-bold text-[var(--color-text-primary)] truncate">
            {plugin.manifest.name}
          </span>
          <span className="text-[10px] text-[var(--color-text-tertiary)]">
            v{plugin.manifest.version}
          </span>
          {plugin.activated && (
            <span className="text-[10px] text-[var(--color-status-green)]">active</span>
          )}
        </div>
        {plugin.manifest.description && (
          <p className="text-[11px] text-[var(--color-text-secondary)] mt-0.5 line-clamp-2">
            {plugin.manifest.description}
          </p>
        )}
        {plugin.manifest.author && (
          <p className="text-[10px] text-[var(--color-text-tertiary)] mt-1">
            by{" "}
            <a
              href={`https://github.com/${plugin.manifest.author}`}
              target="_blank"
              rel="noreferrer"
              className="text-[var(--color-accent-blue)] hover:underline"
            >
              {plugin.manifest.author}
            </a>
          </p>
        )}
      </div>
      <div className="flex items-center gap-1.5 shrink-0">
        <button
          className="p-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer"
          onClick={handleToggle}
          title={plugin.enabled ? "Disable" : "Enable"}
        >
          {plugin.enabled ? <ToggleRight size={16} weight="fill" /> : <ToggleLeft size={16} />}
        </button>
        <button
          className="p-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-status-red)] transition-colors cursor-pointer disabled:opacity-40"
          onClick={handleUninstall}
          disabled={removing}
          title="Uninstall"
        >
          <Trash size={14} />
        </button>
      </div>
    </div>
  );
}

function RegistryPluginCard({ entry, installed }: { entry: RegistryEntry; installed: boolean }) {
  const { install, installing } = usePluginStore(
    useShallow((s) => ({ install: s.install, installing: s.installing })),
  );
  const addToast = useToastStore((s) => s.addToast);
  const entryId = registryEntryId(entry);
  const isInstalling = installing === entryId;

  const handleInstall = async () => {
    try {
      await install(entry);
      addToast({
        message: `Installed "${entry.name}" — restart to activate`,
        type: "success",
      });
    } catch {
      addToast({ message: `Failed to install "${entry.name}"`, type: "error" });
    }
  };

  return (
    <div className="flex items-start gap-3 px-4 py-3 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] shadow-[2px_2px_0_rgba(0,0,0,0.15)] rounded-md">
      <PuzzlePiece
        size={20}
        weight="duotone"
        className="shrink-0 mt-0.5 text-[var(--color-text-tertiary)]"
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-xs font-bold text-[var(--color-text-primary)] truncate">
            {entry.name}
          </span>
          <span className="text-[10px] text-[var(--color-text-tertiary)]">v{entry.version}</span>
        </div>
        {entry.description && (
          <p className="text-[11px] text-[var(--color-text-secondary)] mt-0.5 line-clamp-2">
            {entry.description}
          </p>
        )}
        <div className="flex items-center gap-3 mt-1">
          {entry.author && (
            <span className="text-[10px] text-[var(--color-text-tertiary)]">
              by{" "}
              <a
                href={`https://github.com/${entry.author}`}
                target="_blank"
                rel="noreferrer"
                className="text-[var(--color-accent-blue)] hover:underline"
              >
                {entry.author}
              </a>
            </span>
          )}
          {entry.homepage && (
            <a
              href={entry.homepage}
              target="_blank"
              rel="noreferrer"
              className="text-[10px] text-[var(--color-accent-blue)] hover:underline flex items-center gap-0.5"
            >
              <Globe size={10} />
              repo
            </a>
          )}
        </div>
      </div>
      <div className="shrink-0">
        {installed ? (
          <span className="text-[10px] text-[var(--color-text-tertiary)] px-2 py-1 border border-[var(--color-border-secondary)] rounded-md">
            Installed
          </span>
        ) : (
          <button
            className="flex items-center gap-1 text-[11px] px-2.5 py-1 bg-[var(--color-accent-blue)] text-white hover:brightness-110 transition-all cursor-pointer disabled:opacity-50 rounded-md"
            onClick={handleInstall}
            disabled={isInstalling || installing !== null}
          >
            {isInstalling ? (
              <CircleNotch size={12} className="animate-spin" />
            ) : (
              <DownloadSimple size={12} />
            )}
            Install
          </button>
        )}
      </div>
    </div>
  );
}

export function MarketplaceTab({ tab: _tab, paneId: _paneId }: TabContentProps) {
  const { plugins, registry, fetchRegistry, ready } = usePluginStore(
    useShallow((s) => ({
      plugins: s.plugins,
      registry: s.registry,
      fetchRegistry: s.fetchRegistry,
      ready: s.ready,
    })),
  );
  const [filter, setFilter] = useState("");

  useEffect(() => {
    fetchRegistry();
  }, [fetchRegistry]);

  const installedList = Object.values(plugins);
  const installedIds = new Set(installedList.map((p) => p.pluginId));

  const filteredInstalled = filter
    ? installedList.filter(
        (p) =>
          p.manifest.name.toLowerCase().includes(filter.toLowerCase()) ||
          p.pluginId.toLowerCase().includes(filter.toLowerCase()),
      )
    : installedList;

  const filteredRegistry = filter
    ? registry.filter(
        (e) =>
          e.name.toLowerCase().includes(filter.toLowerCase()) ||
          (e.author?.toLowerCase().includes(filter.toLowerCase()) ?? false),
      )
    : registry;

  if (!ready) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="flex flex-col items-center gap-3 animate-pulse">
          <PuzzlePiece size={24} className="text-[var(--color-text-tertiary)]" />
          <p className="text-xs font-mono text-[var(--color-text-secondary)]">LOADING_PLUGINS...</p>
        </div>
      </div>
    );
  }

  return (
    <ScrollArea className="h-full bg-[var(--color-bg-page)]">
      <div className="max-w-3xl mx-auto py-8 px-6">
        {/* Header */}
        <div className="flex items-center gap-3 mb-6">
          <PuzzlePiece size={20} weight="duotone" className="text-[var(--color-accent-blue)]" />
          <h2 className="text-sm font-bold text-[var(--color-text-primary)]">Extensions</h2>
          <div className="flex-1" />
          <button
            className="p-1.5 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer"
            onClick={() => fetchRegistry()}
            title="Refresh registry"
          >
            <ArrowClockwise size={14} />
          </button>
        </div>

        {/* Search */}
        <input
          type="text"
          placeholder="Search extensions..."
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="w-full text-xs bg-[var(--color-bg-surface)] border border-[var(--color-border-secondary)] text-[var(--color-text-primary)] px-3 py-2 mb-6 outline-none hover:border-[var(--color-border-primary)] focus:border-[var(--color-accent-blue)] transition-colors placeholder:text-[var(--color-text-muted)] rounded-md"
        />

        {/* Installed */}
        <SectionTitle>Installed</SectionTitle>
        <div className="flex flex-col gap-2 mt-2 mb-8">
          {filteredInstalled.length === 0 ? (
            <p className="text-xs text-[var(--color-text-muted)] py-4 text-center">
              {filter ? "No installed extensions match your search" : "No extensions installed"}
            </p>
          ) : (
            filteredInstalled.map((plugin) => (
              <InstalledPluginCard key={plugin.pluginId} plugin={plugin} />
            ))
          )}
        </div>

        {/* Available from Registry */}
        <SectionTitle>Available</SectionTitle>
        <div className="flex flex-col gap-2 mt-2">
          {filteredRegistry.length === 0 ? (
            <p className="text-xs text-[var(--color-text-muted)] py-4 text-center">
              {filter
                ? "No available extensions match your search"
                : "No extensions available from registry"}
            </p>
          ) : (
            filteredRegistry.map((entry) => {
              const id = registryEntryId(entry);
              return <RegistryPluginCard key={id} entry={entry} installed={installedIds.has(id)} />;
            })
          )}
        </div>
      </div>
    </ScrollArea>
  );
}
