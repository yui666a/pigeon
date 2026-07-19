import { useState } from "react";
import { Modal } from "../common/Modal";
import { projectPathString } from "../../stores/projectTree";
import type { Project } from "../../types/project";

interface MergeProjectDialogProps {
  sourceProject: Project;
  candidates: Project[];
  /** パス表記の解決用（candidates に含まれない祖先も要るため全案件を渡す） */
  projects: Project[];
  onMerge: (targetId: string) => Promise<void>;
  onCancel: () => void;
}

export function MergeProjectDialog({
  sourceProject,
  candidates,
  projects,
  onMerge,
  onCancel,
}: MergeProjectDialogProps) {
  const [selectedTargetId, setSelectedTargetId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const handleMerge = async () => {
    if (!selectedTargetId || submitting) return;
    setSubmitting(true);
    try {
      await onMerge(selectedTargetId);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      ariaLabel={`「${sourceProject.name}」を他の案件にマージ`}
      onClose={onCancel}
      className="w-80 p-4"
    >
      <h3 className="text-sm font-semibold">
        「{sourceProject.name}」を他の案件にマージ
      </h3>
      <p className="mt-1 text-xs text-gray-500">
        すべてのメールがマージ先に移動し、この案件は削除されます。
      </p>

      <div className="mt-3 max-h-48 overflow-y-auto border rounded">
        {candidates.length === 0 ? (
          <p className="px-3 py-2 text-xs text-gray-400">
            マージ先の案件がありません
          </p>
        ) : (
          [...candidates]
            .sort((a, b) =>
              projectPathString(projects, a.id).localeCompare(
                projectPathString(projects, b.id),
              ),
            )
            .map((project) => (
            <button
              key={project.id}
              onClick={() => setSelectedTargetId(project.id)}
              className={`w-full px-3 py-2 text-left text-sm hover:bg-gray-50 ${
                selectedTargetId === project.id
                  ? "bg-blue-50 text-blue-700"
                  : "text-gray-700"
              }`}
            >
              {projectPathString(projects, project.id)}
            </button>
            ))
        )}
      </div>

      <div className="mt-3 flex justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded border px-3 py-1 text-sm hover:bg-gray-100"
        >
          キャンセル
        </button>
        <button
          onClick={() => void handleMerge()}
          disabled={!selectedTargetId || submitting}
          className="rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700 disabled:opacity-40"
        >
          マージ
        </button>
      </div>
    </Modal>
  );
}
