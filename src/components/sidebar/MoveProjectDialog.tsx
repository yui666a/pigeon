import { useState } from "react";
import { Modal } from "../common/Modal";
import { useProjectStore } from "../../stores/projectStore";
import { buildProjectTree, collectSubtreeIds } from "../../stores/projectTree";
import type { ProjectTreeNode } from "../../stores/projectTree";

interface MoveProjectDialogProps {
  projectId: string;
  onClose: () => void;
}

export function MoveProjectDialog({ projectId, onClose }: MoveProjectDialogProps) {
  const projects = useProjectStore((s) => s.projects);
  const setProjectParent = useProjectStore((s) => s.setProjectParent);
  const [selectedParentId, setSelectedParentId] = useState<string | null | undefined>(
    undefined,
  );
  const [submitting, setSubmitting] = useState(false);

  const project = projects.find((p) => p.id === projectId);
  const tree = buildProjectTree(projects);
  const disabledIds = collectSubtreeIds(tree, projectId);

  const handleSubmit = async () => {
    if (selectedParentId === undefined || submitting) return;
    setSubmitting(true);
    try {
      await setProjectParent(projectId, selectedParentId);
      onClose();
    } catch {
      // setProjectParent は projectStore 内で errorStore へ通知済み。
      // ダイアログは開いたままにし、別の親を選び直せるようにする
    } finally {
      setSubmitting(false);
    }
  };

  const renderNode = (node: ProjectTreeNode, depth: number) => {
    const disabled = disabledIds.has(node.project.id);
    return (
      <div key={node.project.id}>
        <label
          className={`flex items-center gap-2 px-3 py-1.5 text-sm ${
            disabled ? "text-gray-300" : "cursor-pointer text-gray-700 hover:bg-gray-50"
          }`}
          style={{ paddingLeft: 12 + depth * 16 }}
        >
          <input
            type="radio"
            name="move-project-parent"
            aria-label={node.project.name}
            disabled={disabled}
            checked={selectedParentId === node.project.id}
            onChange={() => setSelectedParentId(node.project.id)}
          />
          {node.project.name}
        </label>
        {node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    );
  };

  if (!project) return null;

  return (
    <Modal
      ariaLabel={`「${project.name}」の親を変更`}
      onClose={onClose}
      className="w-80 p-4"
    >
      <h3 className="text-sm font-semibold">「{project.name}」の親を変更</h3>
      <p className="mt-1 text-xs text-gray-500">
        新しい親案件を選択してください。自分自身と配下の案件は選択できません。
      </p>

      <div className="mt-3 max-h-56 overflow-y-auto rounded border">
        <label className="flex items-center gap-2 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
          <input
            type="radio"
            name="move-project-parent"
            aria-label="ルート（親なし）"
            checked={selectedParentId === null}
            onChange={() => setSelectedParentId(null)}
          />
          ルート（親なし）
        </label>
        {tree.map((node) => renderNode(node, 0))}
      </div>

      <div className="mt-3 flex justify-end gap-2">
        <button
          onClick={onClose}
          className="rounded border px-3 py-1 text-sm hover:bg-gray-100"
        >
          キャンセル
        </button>
        <button
          onClick={() => void handleSubmit()}
          disabled={selectedParentId === undefined || submitting}
          className="rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700 disabled:opacity-40"
        >
          変更
        </button>
      </div>
    </Modal>
  );
}
