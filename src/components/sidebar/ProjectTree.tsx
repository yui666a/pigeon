import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useClassifyStore } from "../../stores/classifyStore";
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
  const [dragOverProjectId, setDragOverProjectId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    projectId: string;
  } | null>(null);

  useEffect(() => {
    if (selectedAccountId) {
      fetchProjects(selectedAccountId);
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchProjects, fetchUnclassified]);

  if (!selectedAccountId) {
    return null;
  }

  const handleProjectContextMenu = (e: React.MouseEvent, projectId: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, projectId });
  };

  const getProjectMenuItems = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return [];
    return [
      {
        label: "名前変更",
        onClick: async () => {
          const newName = window.prompt("案件名を入力", project.name);
          if (newName && newName.trim()) {
            await updateProject(projectId, newName.trim());
            await fetchProjects(selectedAccountId!);
          }
        },
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
          if (window.confirm(`「${project.name}」を削除しますか？この操作は取り消せません。`)) {
            await deleteProject(projectId);
            await fetchProjects(selectedAccountId!);
          }
        },
      },
    ];
  };

  const handleDragOver = (e: React.DragEvent, projectId: string) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDragOverProjectId(projectId);
  };

  const handleDragLeave = () => {
    setDragOverProjectId(null);
  };

  const handleDrop = async (e: React.DragEvent, projectId: string) => {
    e.preventDefault();
    setDragOverProjectId(null);
    const data = e.dataTransfer.getData("application/pigeon-mail-ids");
    if (!data || !selectedAccountId) return;
    const mailIds: string[] = JSON.parse(data);
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
            <button
              onClick={() => {
                selectProject(project.id);
                onSelectProject();
              }}
              onContextMenu={(e) => handleProjectContextMenu(e, project.id)}
              onDragOver={(e) => handleDragOver(e, project.id)}
              onDragLeave={handleDragLeave}
              onDrop={(e) => handleDrop(e, project.id)}
              className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
                selectedProjectId === project.id
                  ? "bg-blue-50 font-semibold text-blue-700"
                  : ""
              } ${dragOverProjectId === project.id ? "bg-blue-100 ring-2 ring-blue-400 ring-inset" : ""}`}
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
