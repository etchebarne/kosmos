import type { Icon } from "@phosphor-icons/react";
import {
  File,
  Code,
  Folder,
  GitBranch,
  GitDiff,
  Terminal,
  GearSix,
  Infinity as InfinityIcon,
  MagnifyingGlass,
  PuzzlePiece,
} from "@phosphor-icons/react";

interface TabIconProps {
  name: string;
  size?: number;
  className?: string;
}

// Map tab icon keys to phosphor icon components
const iconMap: Record<string, Icon> = {
  file: File,
  code: Code,
  "folder-tree": Folder,
  "git-branch": GitBranch,
  "git-compare": GitDiff,
  terminal: Terminal,
  settings: GearSix,
  infinity: InfinityIcon,
  "magnifying-glass": MagnifyingGlass,
  "puzzle-piece": PuzzlePiece,
};

export function TabIcon({ name, size = 14, className }: TabIconProps) {
  const IconComponent = iconMap[name];
  if (!IconComponent) return null;
  return <IconComponent size={size} className={className} />;
}
