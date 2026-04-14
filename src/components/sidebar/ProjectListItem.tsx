import { memo } from "react";
import type { Project } from "../../types/project";

interface ProjectListItemProps {
  project: Project;
  selected: boolean;
  isRenaming: boolean;
  renameValue: string;
  renameInputRef: React.RefObject<HTMLInputElement | null>;
  isDragHover: boolean;
  isDragging: boolean;
  onSelect: () => void;
  onRenameValueChange: (value: string) => void;
  onRenameSubmit: () => void;
  onRenameCancel: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onDragEnter: () => void;
  onDragLeave: () => void;
  onDrop: () => void;
}

export const ProjectListItem = memo(function ProjectListItem({
  project,
  selected,
  isRenaming,
  renameValue,
  renameInputRef,
  isDragHover,
  isDragging,
  onSelect,
  onRenameValueChange,
  onRenameSubmit,
  onRenameCancel,
  onContextMenu,
  onDragEnter,
  onDragLeave,
  onDrop,
}: ProjectListItemProps) {
  if (isRenaming) {
    return (
      <li>
        <form
          onSubmit={(e) => { e.preventDefault(); onRenameSubmit(); }}
          className="px-4 py-1.5"
        >
          <input
            ref={renameInputRef}
            type="text"
            value={renameValue}
            onChange={(e) => onRenameValueChange(e.target.value)}
            onBlur={onRenameSubmit}
            onKeyDown={(e) => { if (e.key === "Escape") onRenameCancel(); }}
            className="w-full rounded border border-blue-400 px-2 py-1 text-sm focus:outline-none"
          />
        </form>
      </li>
    );
  }

  return (
    <li>
      <button
        onClick={() => {
          if (!isDragging) onSelect();
        }}
        onMouseEnter={() => {
          if (isDragging) onDragEnter();
        }}
        onMouseLeave={() => {
          if (isDragging) onDragLeave();
        }}
        onMouseUp={() => {
          if (isDragging) onDrop();
        }}
        onContextMenu={onContextMenu}
        className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
          selected ? "bg-blue-50 font-semibold text-blue-700" : ""
        } ${isDragHover ? "bg-blue-100 ring-2 ring-blue-400 ring-inset" : ""}`}
      >
        <div className="flex items-center gap-2">
          <span
            className="h-2.5 w-2.5 flex-shrink-0 rounded-full"
            style={{ backgroundColor: project.color ?? "#6b7280" }}
          />
          <span className="truncate">{project.name}</span>
        </div>
      </button>
    </li>
  );
});
