import { getVisibleTabDefinitions } from "../registry";
import { useLayoutStore } from "../../store/layout.store";
import { TabIcon } from "../../components/shared/TabIcon";
import { PillButton } from "../../components/shared/PillButton";
import type { TabContentProps } from "../types";

export function BlankTab({ tab, paneId }: TabContentProps) {
  const transformTab = useLayoutStore((s) => s.transformTab);
  const definitions = getVisibleTabDefinitions().filter((d) => d.type !== "blank");

  return (
    <div className="@container relative flex flex-col items-center justify-center h-full w-full overflow-hidden p-4">
      <div
        aria-hidden
        className="absolute inset-0 flex items-center justify-center pointer-events-none"
        style={{ color: "var(--color-logo)", opacity: 0.03 }}
      >
        <svg
          width="360"
          height="300"
          viewBox="0 0 160 133"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
        >
          <path
            d="M81.6304 43.2982C105.663 49.779 96.2076 69.8639 80.8422 78.1903C104.087 74.6223 121.408 45.2811 100.936 33.3861C67.5553 13.9908 26.4722 44.0913 15.4408 57.1757C-3.27985 79.3803 -5.44038 105.153 11.501 120.616C30.8988 138.321 62.1936 131.983 82.0242 124.581C49.3235 125.375 41.0498 119.824 33.9581 112.686C4.40924 82.9488 47.5986 34.1211 81.6304 43.2982Z"
            fill="currentColor"
          />
          <path
            d="M78.3696 88.3487C54.3366 81.8962 63.7924 61.899 79.1578 53.6089C55.9125 57.1614 38.5923 86.3744 59.0643 98.2175C92.4447 117.528 133.528 87.559 144.559 74.5318C163.28 52.4241 165.44 26.7641 148.499 11.3684C129.101 -6.25952 97.8064 0.0515693 77.9759 7.42062C110.677 6.63073 118.95 12.1575 126.042 19.2634C155.591 48.8712 112.401 97.4857 78.3696 88.3487Z"
            fill="currentColor"
          />
        </svg>
      </div>

      <div className="relative flex flex-col items-center gap-6 max-w-full">
        <div className="flex flex-col items-center gap-1.5">
          <h3 className="text-base font-semibold text-[var(--color-text-primary)] tracking-tight">
            New Tab
          </h3>
          <p className="text-xs text-[var(--color-text-secondary)]">
            Choose what to open in this tab.
          </p>
        </div>

        {definitions.length === 0 ? (
          <p className="text-xs text-[var(--color-text-muted)]">No tab types available</p>
        ) : (
          <div className="flex flex-wrap items-center justify-center gap-2 max-w-[440px]">
            {definitions.map((def) => (
              <PillButton
                key={def.type}
                leadingIcon={
                  <TabIcon
                    name={def.icon}
                    size={13}
                    className="shrink-0 text-[var(--color-text-tertiary)]"
                  />
                }
                onClick={() => transformTab(paneId, tab.id, def.type)}
              >
                {def.title}
              </PillButton>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
