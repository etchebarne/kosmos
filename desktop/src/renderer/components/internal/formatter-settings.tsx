import { useEffect } from "react";
import { ArrowDown, ArrowUp, LoaderCircle, RefreshCw, Trash2 } from "lucide-react";

import { Button } from "@/renderer/components/ui/button";
import { useFormatterStore } from "@/renderer/stores";
import type { FormatterSnapshot } from "@/shared/ipc";

export function FormatterSettings() {
  const error = useFormatterStore((state) => state.error);
  const isLoading = useFormatterStore((state) => state.isLoading);
  const formatters = useFormatterStore((state) => state.formatters);
  const pending = useFormatterStore((state) => state.pendingFormatterIds);
  const prioritiesPending = useFormatterStore((state) => state.prioritiesPending);
  const initialize = useFormatterStore((state) => state.initializeFormatters);
  const install = useFormatterStore((state) => state.installFormatter);
  const uninstall = useFormatterStore((state) => state.uninstallFormatter);
  const setPriorities = useFormatterStore((state) => state.setFormatterPriorities);

  const move = (index: number, offset: -1 | 1) => {
    const reordered = formatters.map((formatter) => formatter.id);
    const target = index + offset;
    const currentId = reordered[index];
    const targetId = reordered[target];
    if (currentId === undefined || targetId === undefined) return;
    reordered[index] = targetId;
    reordered[target] = currentId;
    setPriorities(reordered);
  };

  useEffect(() => {
    void initialize();
  }, [initialize]);

  return (
    <section className="scrollbar-themed min-h-0 overflow-y-auto px-5 py-5 sm:px-7">
      <div className="mb-6">
        <h2 className="font-heading text-lg font-medium">Formatters</h2>
        <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
          Installed formatters are tried from top to bottom before language-server formatting.
        </p>
      </div>
      {error ? (
        <div className="mb-4 flex items-center justify-between gap-4 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void initialize()}>
            <RefreshCw /> Retry
          </Button>
        </div>
      ) : null}
      {isLoading && formatters.length === 0 ? (
        <div className="flex min-h-36 items-center justify-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle className="animate-spin" /> Loading formatters...
        </div>
      ) : (
        <ul className="divide-y rounded-xl border bg-card">
          {formatters.map((formatter, index) => (
            <FormatterRow
              key={formatter.id}
              formatter={formatter}
              pending={Boolean(pending[formatter.id])}
              onInstall={() => install(formatter.id)}
              onUninstall={() => uninstall(formatter.id)}
              priorityPending={prioritiesPending}
              canMoveUp={index > 0}
              canMoveDown={index < formatters.length - 1}
              onMoveUp={() => move(index, -1)}
              onMoveDown={() => move(index, 1)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function FormatterRow({
  formatter,
  pending,
  onInstall,
  onUninstall,
  priorityPending,
  canMoveUp,
  canMoveDown,
  onMoveUp,
  onMoveDown,
}: {
  formatter: FormatterSnapshot;
  pending: boolean;
  onInstall(): void;
  onUninstall(): void;
  priorityPending: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onMoveUp(): void;
  onMoveDown(): void;
}) {
  const installed = formatter.installationState === "installed";
  const updateAvailable =
    formatter.installedVersion !== null && formatter.installedVersion !== formatter.catalogVersion;
  const working = pending;
  return (
    <li className="flex flex-col gap-4 p-4 sm:flex-row sm:items-center sm:justify-between" aria-busy={working}>
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="font-medium">{formatter.name}</h3>
          <span className="rounded-full bg-muted px-2 py-0.5 text-[0.7rem] font-medium text-muted-foreground">
            {statusLabel(formatter)}
          </span>
        </div>
        <p className="mt-1 text-sm text-muted-foreground">{formatter.description}</p>
        <p className="mt-2 text-xs text-muted-foreground">
          {formatter.languages.join(", ")} / Catalog {formatter.catalogVersion}
          {formatter.installedVersion ? ` / Installed ${formatter.installedVersion}` : ""}
        </p>
        {formatter.lastError ? (
          <p className="mt-2 text-xs text-destructive" role="alert">{formatter.lastError.message}</p>
        ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <Button type="button" variant="ghost" size="icon-sm" disabled={priorityPending || !canMoveUp} onClick={onMoveUp} aria-label={`Prioritize ${formatter.name}`}>
          <ArrowUp />
        </Button>
        <Button type="button" variant="ghost" size="icon-sm" disabled={priorityPending || !canMoveDown} onClick={onMoveDown} aria-label={`Deprioritize ${formatter.name}`}>
          <ArrowDown />
        </Button>
        {formatter.installedVersion ? (
          <Button type="button" variant="destructive" size="sm" disabled={working} onClick={onUninstall}>
            <Trash2 /> Remove
          </Button>
        ) : null}
        {!installed || updateAvailable ? (
          <Button type="button" size="sm" disabled={working || !formatter.supported} onClick={onInstall}>
            {working ? <LoaderCircle className="animate-spin" /> : null}
            {updateAvailable ? "Update" : formatter.installationState === "failed" ? "Retry" : "Install"}
          </Button>
        ) : null}
      </div>
    </li>
  );
}

function statusLabel(formatter: FormatterSnapshot): string {
  if (!formatter.supported) return "Unsupported";
  switch (formatter.installationState) {
    case "notInstalled": return "Not installed";
    case "installing": return "Installing";
    case "installed": return "Installed";
    case "uninstalling": return "Removing";
    case "failed": return "Failed";
  }
}
