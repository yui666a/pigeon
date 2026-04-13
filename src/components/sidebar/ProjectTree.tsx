import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useClassifyStore } from "../../stores/classifyStore";

interface ProjectTreeProps {
  onSelectUnclassified: () => void;
  onSelectProject: () => void;
}

export function ProjectTree({ onSelectUnclassified, onSelectProject }: ProjectTreeProps) {
  const { selectedAccountId } = useAccountStore();
  const { projects, selectedProjectId, fetchProjects, selectProject } =
    useProjectStore();
  const { unclassifiedMails, fetchUnclassified } = useClassifyStore();

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
      <ul className="flex flex-col">
        {projects.map((project) => (
          <li key={project.id}>
            <button
              onClick={() => {
                selectProject(project.id);
                onSelectProject();
              }}
              className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
                selectedProjectId === project.id
                  ? "bg-blue-50 font-semibold text-blue-700"
                  : ""
              }`}
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
    </div>
  );
}
