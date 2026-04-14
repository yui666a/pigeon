import { useEffect, useRef, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { useDragStore } from "../../stores/dragStore";
import { ContextMenu } from "../common/ContextMenu";

interface ProjectTreeProps {
  onSelectUnclassified: () => void;
  onSelectProject: () => void;
}

export function ProjectTree({ onSelectUnclassified, onSelectProject }: ProjectTreeProps) {
  const { selectedAccountId } = useAccountStore();
  const { projects, selectedProjectId, fetchProjects, selectProject, updateProject, archiveProject, deleteProject } =
    useProjectStore();
  const { unclassifiedMails, fetchUnclassified, moveMail } = useClassifyStore();
  const draggingMailIds = useDragStore((s) => s.draggingMailIds);
  const endDrag = useDragStore((s) => s.endDrag);
  const [hoverProjectId, setHoverProjectId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    projectId: string;
  } | null>(null);
  const [renamingProjectId, setRenamingProjectId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (selectedAccountId) {
      fetchProjects(selectedAccountId);
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchProjects, fetchUnclassified]);

  useEffect(() => {
    if (renamingProjectId && renameInputRef.current) {
      renameInputRef.current.focus();
      renameInputRef.current.select();
    }
  }, [renamingProjectId]);

  // Clear hover highlight when drag ends
  useEffect(() => {
    if (!draggingMailIds) {
      setHoverProjectId(null);
    }
  }, [draggingMailIds]);

  if (!selectedAccountId) {
    return null;
  }

  const handleProjectContextMenu = (e: React.MouseEvent, projectId: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, projectId });
  };

  const startRename = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    setRenamingProjectId(projectId);
    setRenameValue(project.name);
  };

  const submitRename = async () => {
    if (renamingProjectId && renameValue.trim()) {
      await updateProject(renamingProjectId, renameValue.trim());
      await fetchProjects(selectedAccountId!);
    }
    setRenamingProjectId(null);
    setRenameValue("");
  };

  const cancelRename = () => {
    setRenamingProjectId(null);
    setRenameValue("");
  };

  const getProjectMenuItems = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return [];
    return [
      {
        label: "名前変更",
        onClick: () => startRename(projectId),
      },
      {
        label: "アーカイブ",
        onClick: async () => {
          await archiveProject(projectId);
          await fetchProjects(selectedAccountId!);
        },
      },
      {
        label: "削除",
        danger: true,
        onClick: async () => {
          await deleteProject(projectId);
          await fetchProjects(selectedAccountId!);
        },
      },
    ];
  };

  const handleDropOnProject = async (projectId: string) => {
    if (!draggingMailIds || !selectedAccountId) return;
    const mailIds = [...draggingMailIds];
    endDrag();
    for (const mailId of mailIds) {
      await moveMail(mailId, projectId, selectedAccountId);
    }
    await fetchProjects(selectedAccountId);
  };

  return (
    <div className="mt-2">
      <div className="px-4 py-1">
        <span className="text-xs font-semibold uppercase tracking-wide text-gray-400">
          案件
        </span>
      </div>
      <ul className="flex flex-col">
        {projects.map((project) => (
          <li key={project.id}>
            {renamingProjectId === project.id ? (
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
            ) : (
              <button
                onClick={() => {
                  if (draggingMailIds) return;
                  selectProject(project.id);
                  onSelectProject();
                }}
                onMouseEnter={() => {
                  if (draggingMailIds) setHoverProjectId(project.id);
                }}
                onMouseLeave={() => {
                  if (draggingMailIds) setHoverProjectId(null);
                }}
                onMouseUp={() => {
                  if (draggingMailIds) {
                    handleDropOnProject(project.id);
                  }
                }}
                onContextMenu={(e) => handleProjectContextMenu(e, project.id)}
                className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
                  selectedProjectId === project.id
                    ? "bg-blue-50 font-semibold text-blue-700"
                    : ""
                } ${hoverProjectId === project.id ? "bg-blue-100 ring-2 ring-blue-400 ring-inset" : ""}`}
              >
                <div className="flex items-center gap-2">
                  <span
                    className="h-2.5 w-2.5 flex-shrink-0 rounded-full"
                    style={{ backgroundColor: project.color ?? "#6b7280" }}
                  />
                  <span className="truncate">{project.name}</span>
                </div>
              </button>
            )}
          </li>
        ))}
      </ul>
      {projects.length > 0 && <hr className="mx-4 my-1 border-gray-200" />}
      <button
        onClick={onSelectUnclassified}
        className="w-full px-4 py-2 text-left text-sm hover:bg-gray-100"
      >
        <div className="flex items-center gap-2">
          <span className="text-amber-500">!</span>
          <span>未分類</span>
          {unclassifiedMails.length > 0 && (
            <span className="ml-auto rounded-full bg-amber-100 px-1.5 py-0.5 text-xs font-semibold text-amber-600">
              {unclassifiedMails.length}
            </span>
          )}
        </div>
      </button>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={getProjectMenuItems(contextMenu.projectId)}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
