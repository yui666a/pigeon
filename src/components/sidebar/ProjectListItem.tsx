import { memo, useState } from "react";
import type { Project } from "../../types/project";
import { useDragStore } from "../../stores/dragStore";
import { useProjectRenameContext } from "./ProjectRenameContext";

interface ProjectListItemProps {
  project: Project;
  selected: boolean;
  onSelect: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onDrop: (projectId: string) => void;
}

export const ProjectListItem = memo(function ProjectListItem({
  project,
  selected,
  onSelect,
  onContextMenu,
  onDrop,
}: ProjectListItemProps) {
  const {
    renamingProjectId,
    renameValue,
    setRenameValue,
    renameInputRef,
    submitRename,
    cancelRename,
  } = useProjectRenameContext();

  const isDragging = useDragStore((s) => !!s.draggingMailIds);
  const [isHovered, setIsHovered] = useState(false);

  const isRenaming = renamingProjectId === project.id;

  if (isRenaming) {
    return (
      <li>
        <form
          onSubmit={(e) => { e.preventDefault(); submitRename(); }}
          className="px-4 py-1.5"
        >
          <input
            ref={renameInputRef}
            type="text"
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onBlur={submitRename}
            onKeyDown={(e) => { if (e.key === "Escape") cancelRename(); }}
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
          if (isDragging) setIsHovered(true);
        }}
        onMouseLeave={() => {
          if (isDragging) setIsHovered(false);
        }}
        onMouseUp={() => {
          if (isDragging) {
            setIsHovered(false);
            onDrop(project.id);
          }
        }}
        onContextMenu={onContextMenu}
        className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
          selected ? "bg-blue-50 font-semibold text-blue-700" : ""
        } ${isHovered ? "bg-blue-100 ring-2 ring-blue-400 ring-inset" : ""}`}
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
