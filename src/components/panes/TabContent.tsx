import { memo, useState } from "react";
import { getTabDefinition } from "../../tabs/registry";
import { ErrorBoundary } from "../shared/ErrorBoundary";
import type { Tab } from "../../types";

interface TabContentProps {
  tab: Tab;
  paneId: string;
}

function TabContentInner({ tab, paneId }: TabContentProps) {
  const definition = getTabDefinition(tab.type);
  const [mountKey, setMountKey] = useState(0);

  if (!definition) {
    return (
      <div className="font-mono flex items-center justify-center h-full text-[var(--color-text-tertiary)] text-[13px]">
        <span className="px-4 py-2 bg-[var(--color-bg-surface)] border border-[var(--color-border-primary)]">
          Unknown tab type: {tab.type}
        </span>
      </div>
    );
  }

  const Component = definition.component;
  return (
    <ErrorBoundary
      name={`tab:${tab.title}`}
      fallback={(error, reset) => (
        <div className="flex flex-col items-center justify-center h-full gap-3 p-4">
          <p className="text-xs text-[var(--color-status-red)]">
            Tab &ldquo;{tab.title}&rdquo; crashed
          </p>
          <p className="text-xs text-[var(--color-text-muted)] max-w-md text-center break-all">
            {error.message}
          </p>
          <button
            className="text-xs px-3 py-1 bg-[var(--color-bg-surface)] border border-[var(--color-border-primary)] text-[var(--color-text-secondary)] hover:bg-[var(--color-border-primary)]"
            onClick={() => {
              reset();
              setMountKey((k) => k + 1);
            }}
          >
            Reload Tab
          </button>
        </div>
      )}
    >
      <Component key={mountKey} tab={tab} paneId={paneId} />
    </ErrorBoundary>
  );
}

export const TabContent = memo(
  TabContentInner,
  (prev, next) => prev.paneId === next.paneId && prev.tab === next.tab,
);
