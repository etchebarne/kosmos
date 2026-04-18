import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { CaretDown, GearSix } from "@phosphor-icons/react";
import { ScrollArea } from "../../components/shared/ScrollArea";
import { Setting } from "../../components/shared/Setting";
import { Dropdown, type DropdownOption } from "../../components/shared/Dropdown";
import { useSettingsStore } from "../../store/settings.store";
import type { TabContentProps } from "../types";

type SettingControl =
  | { type: "dropdown"; options: DropdownOption[] }
  | { type: "switch" }
  | { type: "number"; min: number; max: number; step: number };

interface SettingEntry {
  key: string;
  label: string;
  description?: string;
  control: SettingControl;
  defaultValue: unknown;
}

interface SettingsGroup {
  title: string;
  settings: SettingEntry[];
}

interface SettingsSection {
  id: string;
  label: string;
  groups: SettingsGroup[];
}

interface SettingsSchema {
  sections: SettingsSection[];
}

function SettingControlRenderer({
  control,
  value,
  onChange,
}: {
  control: SettingControl;
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  switch (control.type) {
    case "dropdown":
      return <Dropdown value={String(value)} options={control.options} onChange={onChange} />;
    case "switch":
      return (
        <button
          className={`relative flex items-center w-8 h-[18px] border transition-colors rounded-full ${
            value
              ? "bg-[var(--color-accent-blue)] border-[var(--color-accent-blue)]"
              : "bg-[var(--color-bg-surface)] border-[var(--color-border-primary)] hover:border-[var(--color-border-hover)]"
          }`}
          onClick={() => onChange(!value)}
        >
          <span
            className={`absolute top-0.5 w-3.5 h-3.5 bg-white transition-transform rounded-full ${
              value ? "left-[16px]" : "left-[2px]"
            }`}
          />
        </button>
      );
    case "number":
      return (
        <input
          type="number"
          value={Number(value)}
          min={control.min}
          max={control.max}
          step={control.step}
          onChange={(e) => onChange(Number(e.target.value))}
          className="text-xs w-16 bg-[var(--color-bg-surface)] border border-[var(--color-border-secondary)] text-[var(--color-text-primary)] px-2 py-1 outline-none hover:border-[var(--color-border-primary)] focus:border-[var(--color-accent-blue)] transition-colors text-center rounded-md"
        />
      );
  }
}

function AccordionSection({
  section,
  expanded,
  onToggle,
  values,
  onChange,
}: {
  section: SettingsSection;
  expanded: boolean;
  onToggle: () => void;
  values: Record<string, unknown>;
  onChange: (key: string, value: unknown) => void;
}) {
  return (
    <div className="mb-4 bg-[var(--color-bg-elevated)] border border-[var(--color-border-primary)] shadow-[2px_2px_0_rgba(0,0,0,0.15)] rounded-md">
      <button
        className={`flex items-center gap-3 w-full px-4 py-3 text-left transition-colors border-b ${
          expanded
            ? "bg-[var(--color-bg-surface)] border-[var(--color-border-primary)] rounded-t-md"
            : "border-transparent hover:bg-[var(--color-bg-hover)] rounded-md"
        }`}
        onClick={onToggle}
      >
        <CaretDown
          size={14}
          className={`text-[var(--color-text-tertiary)] transition-transform duration-200 ${
            expanded ? "" : "-rotate-90"
          }`}
        />
        <span className="text-xs font-bold text-[var(--color-text-primary)]">{section.label}</span>
      </button>

      {expanded && (
        <div className="p-2 bg-[var(--color-bg-page)] rounded-b-md">
          {section.groups.map((group, groupIdx) => (
            <div key={group.title} className={`flex flex-col ${groupIdx > 0 ? "mt-4" : ""}`}>
              <h4 className="text-xs font-bold text-[var(--color-text-secondary)] px-3 pt-3 pb-1">
                {group.title}
              </h4>
              <div className="flex flex-col">
                {group.settings.map((entry) => (
                  <Setting key={entry.key} label={entry.label} description={entry.description}>
                    <SettingControlRenderer
                      control={entry.control}
                      value={values[entry.key] ?? entry.defaultValue}
                      onChange={(v) => onChange(entry.key, v)}
                    />
                  </Setting>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function SettingsTab({ tab: _tab, paneId: _paneId }: TabContentProps) {
  const [schema, setSchema] = useState<SettingsSchema | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [expandedSections, setExpandedSections] = useState<Set<string>>(new Set());
  const values = useSettingsStore((s) => s.values);
  const setSetting = useSettingsStore((s) => s.set);

  useEffect(() => {
    invoke<SettingsSchema>("get_settings_schema").then((s) => {
      setSchema(s);
      setExpandedSections(new Set(s.sections.map((sec) => sec.id)));
    });
    getVersion().then(setVersion);
  }, []);

  const toggleSection = useCallback((id: string) => {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const handleChange = useCallback(
    (key: string, value: unknown) => {
      setSetting(key, value);
    },
    [setSetting],
  );

  if (!schema) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="flex flex-col items-center gap-3 animate-pulse">
          <GearSix size={24} className="text-[var(--color-text-tertiary)] animate-spin-slow" />
          <p className="text-xs font-mono text-[var(--color-text-secondary)]">
            LOADING_SETTINGS...
          </p>
        </div>
      </div>
    );
  }

  return (
    <ScrollArea className="h-full bg-[var(--color-bg-page)]">
      <div className="max-w-3xl mx-auto py-8 px-6 @container">
        <div className="flex flex-col gap-3">
          {schema.sections.map((section) => (
            <AccordionSection
              key={section.id}
              section={section}
              expanded={expandedSections.has(section.id)}
              onToggle={() => toggleSection(section.id)}
              values={values}
              onChange={handleChange}
            />
          ))}
        </div>
        {version && (
          <p className="mt-6 text-center text-[11px] text-[var(--color-text-muted)]">
            Kosmos v{version}
          </p>
        )}
      </div>
    </ScrollArea>
  );
}
