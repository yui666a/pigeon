import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useMailStore } from "../../stores/mailStore";
import { useDragStore } from "../../stores/dragStore";
import { ProjectListItem } from "./ProjectListItem";
import { ProjectRenameProvider, useProjectRenameContext } from "./ProjectRenameContext";
import { ContextMenu } from "../common/ContextMenu";
import { MergeProjectDialog } from "./MergeProjectDialog";

interface ProjectTreeProps {
  onSelectUnclassified: () => void;
  onSelectProject: () => void;
}

export function ProjectTree({ onSelectUnclassified, onSelectProject }: ProjectTreeProps) {
  const { selectedAccountId } = useAccountStore();
  const { projects, fetchProjects } = useProjectStore();
  const { unclassifiedMails, fetchUnclassified } = useMailStore();

  useEffect(() => {
    if (selectedAccountId) {
      fetchProjects(selectedAccountId);
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchProjects, fetchUnclassified]);

  if (!selectedAccountId) {
    return null;
  }

  return (
    <div className="mt-2">
      <div className="px-4 py-1">
        <span className="text-xs font-semibold uppercase tracking-wide text-gray-400">
          案件
        </span>
      </div>
      <ProjectRenameProvider projects={projects}>
        <ProjectListInner
          onSelectProject={onSelectProject}
        />
      </ProjectRenameProvider>
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
    </div>
  );
}

function ProjectListInner({
  onSelectProject,
}: {
  onSelectProject: () => void;
}) {
  const { projects, selectedProjectId, selectProject, archiveProject, deleteProject, mergeProject } =
    useProjectStore();
  const draggingMailIds = useDragStore((s) => s.draggingMailIds);
  const endDrag = useDragStore((s) => s.endDrag);
  const { moveMail } = useMailStore();
  const { startRename } = useProjectRenameContext();
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    projectId: string;
  } | null>(null);
  const [mergeSourceId, setMergeSourceId] = useState<string | null>(null);

  const handleDropOnProject = async (projectId: string) => {
    if (!draggingMailIds) return;
    const mailIds = [...draggingMailIds];
    endDrag();
    for (const mailId of mailIds) {
      await moveMail(mailId, projectId);
    }
  };

  const getProjectMenuItems = (projectId: string) => [
    {
      label: "名前変更",
      onClick: () => startRename(projectId),
    },
    {
      label: "マージ",
      onClick: () => setMergeSourceId(projectId),
    },
    {
      label: "アーカイブ",
      onClick: async () => {
        await archiveProject(projectId);
      },
    },
    {
      label: "削除",
      danger: true,
      onClick: async () => {
        await deleteProject(projectId);
      },
    },
  ];

  return (
    <>
      <ul className="flex flex-col">
        {projects.map((project) => (
          <ProjectListItem
            key={project.id}
            project={project}
            selected={selectedProjectId === project.id}
            onSelect={() => {
              selectProject(project.id);
              onSelectProject();
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              setContextMenu({ x: e.clientX, y: e.clientY, projectId: project.id });
            }}
            onDrop={handleDropOnProject}
          />
        ))}
      </ul>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={getProjectMenuItems(contextMenu.projectId)}
          onClose={() => setContextMenu(null)}
        />
      )}

      {mergeSourceId && (() => {
        const sourceProject = projects.find((p) => p.id === mergeSourceId);
        if (!sourceProject) return null;
        const candidates = projects.filter((p) => p.id !== mergeSourceId);
        return (
          <MergeProjectDialog
            sourceProject={sourceProject}
            candidates={candidates}
            onMerge={async (targetId) => {
              await mergeProject(mergeSourceId, targetId);
              setMergeSourceId(null);
            }}
            onCancel={() => setMergeSourceId(null)}
          />
        );
      })()}
    </>
  );
}
