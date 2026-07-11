import { useState } from "react";
import { Minus, Plus } from "lucide-react";

import { Button } from "@/renderer/components/ui/button";
import { ButtonGroup } from "@/renderer/components/ui/button-group";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/renderer/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/renderer/components/ui/select";
import { Switch } from "@/renderer/components/ui/switch";
import { cn } from "@/renderer/lib/utils";
import { useSettingsStore } from "@/renderer/stores";
import type {
  SettingCategory,
  SettingDefinition,
  SettingItem,
  SettingValue,
} from "@/shared/ipc";

type SettingsDialogProps = {
  open: boolean;
  onOpenChange(open: boolean): void;
};

export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  const [selectedCategoryId, setSelectedCategoryId] = useState<string | null>(null);
  const error = useSettingsStore((state) => state.error);
  const isLoading = useSettingsStore((state) => state.isLoading);
  const pendingSettingIds = useSettingsStore((state) => state.pendingSettingIds);
  const snapshot = useSettingsStore((state) => state.snapshot);
  const updateSetting = useSettingsStore((state) => state.updateSetting);
  const categories = snapshot?.categories ?? [];
  const selectedCategory =
    categories.find((category) => category.id === selectedCategoryId) ?? categories[0];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[min(42rem,calc(100vh-2rem))] flex-col gap-0 overflow-hidden p-0 sm:max-w-4xl">
        <DialogHeader className="shrink-0 border-b px-5 py-4 pr-12">
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>Configure Kosmos. Changes are saved automatically.</DialogDescription>
        </DialogHeader>

        {isLoading && !snapshot ? (
          <div className="grid min-h-0 flex-1 place-items-center text-sm text-muted-foreground">
            Loading settings...
          </div>
        ) : categories.length === 0 ? (
          <div className="grid min-h-0 flex-1 place-items-center text-sm text-muted-foreground">
            No settings are available.
          </div>
        ) : (
          <div className="grid min-h-0 flex-1 grid-rows-[auto_1fr] sm:grid-cols-[12rem_1fr] sm:grid-rows-1">
            <nav
              aria-label="Settings categories"
              className="scrollbar-themed flex gap-1 overflow-auto border-b bg-muted/30 p-2 sm:flex-col sm:border-r sm:border-b-0"
            >
              {categories.map((category) => (
                <Button
                  key={category.id}
                  type="button"
                  variant={category.id === selectedCategory?.id ? "secondary" : "ghost"}
                  className="shrink-0 justify-start"
                  onClick={() => setSelectedCategoryId(category.id)}
                >
                  {category.label}
                </Button>
              ))}
            </nav>

            {selectedCategory ? (
              <CategoryContent
                category={selectedCategory}
                pendingSettingIds={pendingSettingIds}
                onUpdate={updateSetting}
              />
            ) : null}
          </div>
        )}

        <DialogFooter className="mx-0 mb-0 shrink-0 flex-row items-center justify-between rounded-none px-5 py-3">
          <p className="min-w-0 truncate text-xs text-destructive" role={error ? "alert" : undefined}>
            {error}
          </p>
          <DialogClose render={<Button type="button" variant="outline" />}>Close</DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function CategoryContent({
  category,
  pendingSettingIds,
  onUpdate,
}: {
  category: SettingCategory;
  pendingSettingIds: Record<string, true>;
  onUpdate(id: string, value: SettingValue): void;
}) {
  return (
    <section className="scrollbar-themed min-h-0 overflow-y-auto px-5 py-5 sm:px-7">
      <div className="mb-6">
        <h2 className="font-heading text-lg font-medium">{category.label}</h2>
        {category.description ? (
          <p className="mt-1 text-sm text-muted-foreground">{category.description}</p>
        ) : null}
      </div>
      <SettingItems
        items={category.items}
        pendingSettingIds={pendingSettingIds}
        onUpdate={onUpdate}
      />
    </section>
  );
}

function SettingItems({
  items,
  pendingSettingIds,
  onUpdate,
  nested = false,
}: {
  items: SettingItem[];
  pendingSettingIds: Record<string, true>;
  onUpdate(id: string, value: SettingValue): void;
  nested?: boolean;
}) {
  return (
    <div className={cn("divide-y", nested && "ml-3 border-l pl-5")}>
      {items.map((item) =>
        item.type === "group" ? (
          <section key={item.id} className="py-5 first:pt-0 last:pb-0">
            <h3 className="font-medium">{item.label}</h3>
            {item.description ? (
              <p className="mt-1 mb-4 text-sm text-muted-foreground">{item.description}</p>
            ) : null}
            <SettingItems
              items={item.items}
              pendingSettingIds={pendingSettingIds}
              onUpdate={onUpdate}
              nested
            />
          </section>
        ) : (
          <SettingRow
            key={item.id}
            setting={item}
            isPending={Boolean(pendingSettingIds[item.id])}
            onUpdate={onUpdate}
          />
        ),
      )}
    </div>
  );
}

function SettingRow({
  setting,
  isPending,
  onUpdate,
}: {
  setting: SettingDefinition;
  isPending: boolean;
  onUpdate(id: string, value: SettingValue): void;
}) {
  return (
    <div
      className="flex min-h-16 items-center justify-between gap-6 py-4 first:pt-0 last:pb-0"
      aria-busy={isPending}
    >
      <div className="min-w-0">
        <label className="font-medium" htmlFor={setting.id}>
          {setting.label}
        </label>
        {setting.description ? (
          <p className="mt-1 max-w-xl text-sm leading-relaxed text-muted-foreground">
            {setting.description}
          </p>
        ) : null}
      </div>
      <SettingControl
        setting={setting}
        onUpdate={(value) => onUpdate(setting.id, value)}
      />
    </div>
  );
}

function SettingControl({
  setting,
  onUpdate,
}: {
  setting: SettingDefinition;
  onUpdate(value: SettingValue): void;
}) {
  const control = setting.control;

  if (control.type === "switch") {
    return (
      <Switch
        id={setting.id}
        checked={typeof setting.value === "boolean" ? setting.value : false}
        onCheckedChange={onUpdate}
      />
    );
  }

  if (control.type === "select") {
    return (
      <Select
        value={typeof setting.value === "string" ? setting.value : null}
        onValueChange={(value) => value !== null && onUpdate(value)}
      >
        <SelectTrigger id={setting.id} className="min-w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {control.options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (control.inputType === "number") {
    const value = typeof setting.value === "number" ? setting.value : 0;
    const step = control.step ?? 1;

    return (
      <ButtonGroup className="w-32 shrink-0" aria-label={setting.label}>
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label={`Decrease ${setting.label}`}
          disabled={control.min !== null && control.min !== undefined && value <= control.min}
          onClick={() => onUpdate(clampNumber(value - step, control.min, control.max))}
        >
          <Minus />
        </Button>
        <input
          key={`${setting.id}:${setting.value}`}
          id={setting.id}
          type="number"
          defaultValue={value}
          min={control.min ?? undefined}
          max={control.max ?? undefined}
          step={step}
          data-slot="input"
          className="h-8 min-w-0 flex-1 border border-input bg-transparent px-1 text-center text-sm font-medium tabular-nums outline-none [appearance:textfield] focus-visible:relative focus-visible:z-10 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.currentTarget.blur();
            }
          }}
          onBlur={(event) => {
            const enteredValue = event.currentTarget.valueAsNumber;
            if (!Number.isFinite(enteredValue)) {
              event.currentTarget.value = String(value);
              return;
            }

            const nextValue = clampNumber(enteredValue, control.min, control.max);
            if (nextValue !== value) {
              onUpdate(nextValue);
            } else {
              event.currentTarget.value = String(value);
            }
          }}
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label={`Increase ${setting.label}`}
          disabled={control.max !== null && control.max !== undefined && value >= control.max}
          onClick={() => onUpdate(clampNumber(value + step, control.min, control.max))}
        >
          <Plus />
        </Button>
      </ButtonGroup>
    );
  }

  return (
    <input
      key={`${setting.id}:${setting.value}`}
      id={setting.id}
      type="text"
      defaultValue={String(setting.value)}
      placeholder={control.placeholder ?? undefined}
      min={control.min ?? undefined}
      max={control.max ?? undefined}
      step={control.step ?? undefined}
      className="h-8 w-44 rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50 dark:bg-input/30"
      onBlur={(event) => {
        const value = event.currentTarget.value;
        if (value !== setting.value) {
          onUpdate(value);
        }
      }}
    />
  );
}

function clampNumber(value: number, min?: number | null, max?: number | null): number {
  return Math.min(max ?? Number.POSITIVE_INFINITY, Math.max(min ?? Number.NEGATIVE_INFINITY, value));
}
