import { useState } from "react";

import { Button } from "@/renderer/components/ui/button";
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

  return (
    <input
      key={`${setting.id}:${setting.value}`}
      id={setting.id}
      type={control.inputType}
      defaultValue={String(setting.value)}
      placeholder={control.placeholder ?? undefined}
      min={control.min ?? undefined}
      max={control.max ?? undefined}
      step={control.step ?? undefined}
      className="h-8 w-44 rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50 dark:bg-input/30"
      onBlur={(event) => {
        const value =
          control.inputType === "number" ? event.currentTarget.valueAsNumber : event.currentTarget.value;
        if (typeof value === "number" && !Number.isFinite(value)) {
          return;
        }
        if (value !== setting.value) {
          onUpdate(value);
        }
      }}
    />
  );
}
