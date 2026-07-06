import type { LucideIcon } from "lucide-react";

import { Button } from "@/renderer/components/ui/button";
import type { TabKind } from "@/shared/ipc";

export type BlankTabOption = {
  icon: LucideIcon;
  kind: TabKind;
  label: string;
};

type BlankTabProps = {
  options: BlankTabOption[];
  onActivatePane(): void;
  onSelectKind(kind: TabKind): void;
};

export function BlankTab({ options, onActivatePane, onSelectKind }: BlankTabProps) {
  return (
    <div
      className="grid h-full min-h-0 place-items-center overflow-auto p-6"
      onPointerDown={onActivatePane}
    >
      <div className="flex w-full max-w-xs flex-col gap-2">
        {options.map((option) => {
          const Icon = option.icon;

          return (
            <Button
              key={option.kind}
              type="button"
              variant="outline"
              className="justify-start"
              onPointerDown={(event) => event.stopPropagation()}
              onClick={() => onSelectKind(option.kind)}
            >
              <Icon />
              {option.label}
            </Button>
          );
        })}
      </div>
    </div>
  );
}
