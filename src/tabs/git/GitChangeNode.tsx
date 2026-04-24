import { useState, useCallback, useRef } from "react";
import { Folder, FolderOpen } from "@phosphor-icons/react";
import { getNodeFiles } from "../../lib/gitTree";
import type { TreeNode } from "../../lib/gitTree";
import { useDragStore } from "../../store/drag.store";
import { startDragThreshold } from "../../lib/dragThreshold";
import { gitStatusColor } from "../../lib/gitColors";
import { getFileExtension } from "../../lib/pathUtils";
import { useIsDarkTheme } from "../../lib/themes";
import { FileIcon } from "../fileTree/fileIcons";

interface GitChangeNodeProps {
  node: TreeNode;
  depth: number;
  isUntracked: boolean;
  onToggleStage: (node: TreeNode) => void;
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void;
  onFileClick: (node: TreeNode) => void;
}

const INDENT_SIZE = 16;
const LEFT_PAD = 12;

function getCheckState(node: TreeNode): "checked" | "unchecked" | "indeterminate" {
  const files = getNodeFiles(node);
  if (files.length === 0) return "unchecked";
  const allStaged = files.every((f) => f.staged);
  const someStaged = files.some((f) => f.staged);
  if (allStaged) return "checked";
  if (someStaged) return "indeterminate";
  return "unchecked";
}

function Checkbox({
  state,
  onClick,
}: {
  state: "checked" | "unchecked" | "indeterminate";
  onClick: () => void;
}) {
  return (
    <button
      className="w-3.5 h-3.5 border border-[var(--color-border-secondary)] rounded-none flex items-center justify-center shrink-0 cursor-pointer hover:border-[var(--color-accent-blue)] transition-colors focus:outline-none focus:border-[var(--color-accent-blue)]"
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {state === "checked" && (
        <span className="w-2 h-2 bg-[var(--color-accent-blue)] rounded-none" />
      )}
      {state === "indeterminate" && (
        <span className="w-2 h-0.5 bg-[var(--color-accent-blue)] rounded-none" />
      )}
    </button>
  );
}

export function GitChangeNode({
  node,
  depth,
  isUntracked,
  onToggleStage,
  onContextMenu,
  onFileClick,
}: GitChangeNodeProps) {
  const [expanded, setExpanded] = useState(true);
  const checkState = getCheckState(node);
  const setDragState = useDragStore((s) => s.setDragState);
  const dragOccurredRef = useRef(false);
  const isDark = useIsDarkTheme();

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0 || node.isDir || !node.change) return;

      dragOccurredRef.current = false;

      startDragThreshold(
        e.clientX,
        e.clientY,
        () => {
          dragOccurredRef.current = true;
          setDragState({
            type: "changes",
            filePath: node.change!.path,
            fileName: node.name,
            staged: node.change!.staged,
            isUntracked,
          });
        },
        () => {}, // click is handled by the onClick handler
      );
    },
    [node, isUntracked, setDragState],
  );

  const DirIcon = expanded ? FolderOpen : Folder;

  return (
    <div>
      <div
        className="relative flex items-center w-full h-[28px] gap-1.5 text-left hover:bg-[var(--color-bg-elevated)] focus-within:bg-[var(--color-bg-elevated)] transition-colors select-none cursor-pointer group"
        style={{ paddingLeft: LEFT_PAD + depth * INDENT_SIZE }}
        onClick={() => {
          if (dragOccurredRef.current) return;
          if (node.isDir) setExpanded((prev) => !prev);
          else onFileClick(node);
        }}
        onMouseDown={handleMouseDown}
        onContextMenu={(e) => onContextMenu(e, node)}
      >
        {/* Indent guide lines */}
        {Array.from({ length: depth }, (_, i) => (
          <span
            key={i}
            className="absolute top-0 bottom-0 w-px bg-[var(--color-border-primary)] opacity-40"
            style={{ left: LEFT_PAD + i * INDENT_SIZE + 8 }}
          />
        ))}

        {/* Icon */}
        <span className="w-4 h-4 flex items-center justify-center shrink-0">
          {node.isDir ? (
            <DirIcon size={14} className="shrink-0 text-[var(--color-accent-blue)]" />
          ) : (
            <FileIcon
              name={node.name}
              extension={getFileExtension(node.name)}
              size={14}
              className={`shrink-0 ${node.change ? gitStatusColor(node.change.status) : "text-[var(--color-text-tertiary)]"}`}
              isDark={isDark}
            />
          )}
        </span>

        {/* Name */}
        <span className="text-[13px] text-[var(--color-text-primary)] truncate pb-[1px] flex-1">
          {node.name}
        </span>

        {/* Diff stats (only for files) */}
        {!node.isDir && node.change && (node.change.additions > 0 || node.change.deletions > 0) && (
          <span className="flex items-center gap-1.5 text-[11px] mr-1">
            {node.change.additions > 0 && (
              <span className="text-[var(--color-status-green)]">+ {node.change.additions}</span>
            )}
            {node.change.deletions > 0 && (
              <span className="text-[var(--color-status-red)]">
                &minus; {node.change.deletions}
              </span>
            )}
          </span>
        )}

        {/* Checkbox */}
        <span className="mr-2">
          <Checkbox state={checkState} onClick={() => onToggleStage(node)} />
        </span>
      </div>

      {/* Children */}
      {node.isDir && expanded && (
        <div className="relative">
          {node.children.map((child) => (
            <GitChangeNode
              key={child.path}
              node={child}
              depth={depth + 1}
              isUntracked={isUntracked}
              onToggleStage={onToggleStage}
              onContextMenu={onContextMenu}
              onFileClick={onFileClick}
            />
          ))}
        </div>
      )}
    </div>
  );
}
