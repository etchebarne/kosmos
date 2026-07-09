import type { ReactNode } from "react";
import {
  File,
  FileDiff,
  FileText,
  FolderTree,
  GitBranch,
  Search,
  Terminal as TerminalIcon,
  type LucideIcon,
} from "lucide-react";

import type { PaneId, TabKind, TabSnapshot, WorkspaceId } from "@/shared/ipc";

import { BlankTab, type BlankTabOption } from "./blank";
import { DiffTab } from "./diff";
import { FileTreeTab } from "./file-tree";
import { GitTab } from "./git";
import { PlaceholderTab } from "./placeholder";
import { TerminalTab } from "./terminal";

type TabContentProps = {
  paneId: PaneId;
  tab: TabSnapshot;
  workspaceId: WorkspaceId;
  isActive: boolean;
  onActivatePane(): void;
  onSetTabKind(kind: TabKind): void;
};

type TabDefinition = {
  icon: LucideIcon;
  label: string;
  showInBlankPicker: boolean;
  render(props: TabContentProps): ReactNode;
};

const TAB_KIND_ORDER: TabKind[] = [
  "blank",
  "fileTree",
  "editor",
  "git",
  "search",
  "terminal",
];

const TAB_DEFINITIONS: Record<TabKind, TabDefinition> = {
  blank: {
    icon: File,
    label: "Blank",
    showInBlankPicker: false,
    render: ({ onActivatePane, onSetTabKind }) => (
      <BlankTab
        options={BLANK_TAB_OPTIONS}
        onActivatePane={onActivatePane}
        onSelectKind={onSetTabKind}
      />
    ),
  },
  diff: {
    icon: FileDiff,
    label: "Diff",
    showInBlankPicker: false,
    render: ({ tab, workspaceId, isActive, onActivatePane }) => (
      <DiffTab
        workspaceId={workspaceId}
        tabId={tab.id}
        isActive={isActive}
        onActivatePane={onActivatePane}
      />
    ),
  },
  fileTree: {
    icon: FolderTree,
    label: "File Tree",
    showInBlankPicker: true,
    render: ({ tab, workspaceId, onActivatePane }) => (
      <FileTreeTab workspaceId={workspaceId} tabId={tab.id} onActivatePane={onActivatePane} />
    ),
  },
  editor: placeholderTabDefinition("Editor", FileText, false),
  git: {
    icon: GitBranch,
    label: "Git",
    showInBlankPicker: true,
    render: ({ tab, workspaceId, onActivatePane }) => (
      <GitTab workspaceId={workspaceId} tabId={tab.id} onActivatePane={onActivatePane} />
    ),
  },
  search: placeholderTabDefinition("Search", Search, true),
  terminal: {
    icon: TerminalIcon,
    label: "Terminal",
    showInBlankPicker: true,
    render: ({ tab, workspaceId, isActive, onActivatePane }) => (
      <TerminalTab
        workspaceId={workspaceId}
        tabId={tab.id}
        isActive={isActive}
        onActivatePane={onActivatePane}
      />
    ),
  },
};

const BLANK_TAB_OPTIONS: BlankTabOption[] = TAB_KIND_ORDER.filter(
  (kind) => TAB_DEFINITIONS[kind].showInBlankPicker,
).map((kind) => ({
  icon: TAB_DEFINITIONS[kind].icon,
  kind,
  label: TAB_DEFINITIONS[kind].label,
}));

export function renderTabContent(props: TabContentProps): ReactNode {
  return TAB_DEFINITIONS[props.tab.kind].render(props);
}

export function tabKindIcon(kind: TabKind): LucideIcon {
  return TAB_DEFINITIONS[kind].icon;
}

function placeholderTabDefinition(
  label: string,
  icon: LucideIcon,
  showInBlankPicker: boolean,
): TabDefinition {
  return {
    icon,
    label,
    showInBlankPicker,
    render: ({ tab, onActivatePane }) => (
      <PlaceholderTab title={tab.title} onActivatePane={onActivatePane} />
    ),
  };
}
