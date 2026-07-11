import { useEffect } from "react";
import { LoaderCircle, RefreshCw, Trash2 } from "lucide-react";

import { Button } from "@/renderer/components/ui/button";
import { useLanguageServerStore } from "@/renderer/stores";
import type { LanguageServerSnapshot } from "@/shared/ipc";

export function LanguageServerSettings() {
  const error = useLanguageServerStore((state) => state.error);
  const isLoading = useLanguageServerStore((state) => state.isLoading);
  const servers = useLanguageServerStore((state) => state.servers);
  const pendingServerIds = useLanguageServerStore((state) => state.pendingServerIds);
  const initializeLanguageServers = useLanguageServerStore(
    (state) => state.initializeLanguageServers,
  );
  const installLanguageServer = useLanguageServerStore((state) => state.installLanguageServer);
  const uninstallLanguageServer = useLanguageServerStore((state) => state.uninstallLanguageServer);
  const restartLanguageServer = useLanguageServerStore((state) => state.restartLanguageServer);

  useEffect(() => {
    void initializeLanguageServers();
  }, [initializeLanguageServers]);

  return (
    <section className="scrollbar-themed min-h-0 overflow-y-auto px-5 py-5 sm:px-7">
      <div className="mb-6">
        <h2 className="font-heading text-lg font-medium">Language Servers</h2>
        <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
          Install reviewed, version-pinned language servers managed by Kosmos. Servers are never
          downloaded automatically.
        </p>
      </div>

      {error ? (
        <div className="mb-4 flex items-center justify-between gap-4 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive" role="alert">
          <span>{error}</span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => void initializeLanguageServers()}
          >
            <RefreshCw />
            Retry
          </Button>
        </div>
      ) : null}

      {isLoading && servers.length === 0 ? (
        <div className="flex min-h-36 items-center justify-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle className="animate-spin" />
          Loading language servers...
        </div>
      ) : servers.length === 0 ? (
        <p className="py-10 text-center text-sm text-muted-foreground">
          No language servers are available for this build.
        </p>
      ) : (
        <ul className="divide-y rounded-xl border bg-card">
          {servers.map((server) => (
            <LanguageServerRow
              key={server.id}
              server={server}
              pending={Boolean(pendingServerIds[server.id])}
              onInstall={() => installLanguageServer(server.id)}
              onUninstall={() => uninstallLanguageServer(server.id)}
              onRestart={() => restartLanguageServer(server.id)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function LanguageServerRow({
  server,
  pending,
  onInstall,
  onUninstall,
  onRestart,
}: {
  server: LanguageServerSnapshot;
  pending: boolean;
  onInstall(): void;
  onUninstall(): void;
  onRestart(): void;
}) {
  const installed = server.installationState === "installed";
  const updateAvailable =
    server.installedVersion !== null && server.installedVersion !== server.catalogVersion;
  const working =
    pending ||
    server.installationState === "installing" ||
    server.installationState === "uninstalling";

  return (
    <li className="flex flex-col gap-4 p-4 sm:flex-row sm:items-center sm:justify-between" aria-busy={working}>
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="font-medium">{server.name}</h3>
          <span className="rounded-full bg-muted px-2 py-0.5 text-[0.7rem] font-medium text-muted-foreground">
            {statusLabel(server)}
          </span>
          {installed ? (
            <span className="rounded-full bg-muted px-2 py-0.5 text-[0.7rem] font-medium text-muted-foreground">
              {runtimeStatusLabel(server)}
            </span>
          ) : null}
        </div>
        <p className="mt-1 text-sm text-muted-foreground">{server.description}</p>
        <p className="mt-2 text-xs text-muted-foreground">
          {server.languages.join(", ")} / Catalog {server.catalogVersion}
          {server.installedVersion ? ` / Installed ${server.installedVersion}` : ""}
          {server.sessionCount > 0
            ? ` / ${server.sessionCount} session${server.sessionCount === 1 ? "" : "s"} in ${server.workspaceCount} workspace${server.workspaceCount === 1 ? "" : "s"}`
            : ""}
        </p>
        {server.lastError ? (
          <p className="mt-2 text-xs text-destructive" role="alert">
            {server.lastError.message}
          </p>
        ) : null}
        {server.runtimeError ? (
          <p className="mt-2 text-xs text-destructive" role="alert">
            {server.runtimeError.message}
          </p>
        ) : null}
      </div>

      <div className="flex shrink-0 items-center gap-2">
        {server.installedVersion ? (
          <Button type="button" variant="outline" size="sm" disabled={working} onClick={onRestart}>
            <RefreshCw />
            Restart
          </Button>
        ) : null}
        {server.installedVersion ? (
          <Button
            type="button"
            variant="destructive"
            size="sm"
            disabled={working}
            onClick={onUninstall}
          >
            <Trash2 />
            Remove
          </Button>
        ) : null}
        {!installed || updateAvailable ? (
          <Button
            type="button"
            size="sm"
            className="min-w-20"
            disabled={working || !server.supported}
            onClick={onInstall}
          >
            {working ? <LoaderCircle className="animate-spin" /> : null}
            {updateAvailable ? "Update" : server.installationState === "failed" ? "Retry" : "Install"}
          </Button>
        ) : null}
      </div>
    </li>
  );
}

function runtimeStatusLabel(server: LanguageServerSnapshot): string {
  switch (server.runtimeState) {
    case "inactive":
      return "Idle";
    case "running":
      return "Running";
    case "degraded":
      return "Degraded";
    case "crashed":
      return "Crashed";
  }
}

function statusLabel(server: LanguageServerSnapshot): string {
  if (!server.supported) {
    return "Unsupported";
  }

  switch (server.installationState) {
    case "notInstalled":
      return "Not installed";
    case "installing":
      return "Installing";
    case "installed":
      return "Installed";
    case "uninstalling":
      return "Removing";
    case "failed":
      return "Failed";
  }
}
