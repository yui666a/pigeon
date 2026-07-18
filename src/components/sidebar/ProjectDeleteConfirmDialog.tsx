import { useState } from "react";
import { Modal } from "../common/Modal";
import type { DeleteImpact, Project } from "../../types/project";

interface ProjectDeleteConfirmDialogProps {
  project: Project;
  impact: DeleteImpact;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
}

export function ProjectDeleteConfirmDialog({
  project,
  impact,
  onConfirm,
  onCancel,
}: ProjectDeleteConfirmDialogProps) {
  const [submitting, setSubmitting] = useState(false);

  const handleConfirm = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      await onConfirm();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal ariaLabel={`「${project.name}」を削除`} onClose={onCancel} className="w-80 p-4">
      <h3 className="text-sm font-semibold">「{project.name}」を削除</h3>
      <p className="mt-2 text-xs text-gray-600">
        配下の案件 {impact.projects} 件・メール {impact.mails} 件が対象になります。
      </p>
      <p className="mt-2 text-xs text-gray-500">
        配下のメールは未分類に戻ります。同じスレッドに他案件のメールがある場合、AIが再分類することがあります。
      </p>

      <div className="mt-3 flex justify-end gap-2">
        <button
          onClick={onCancel}
          className="rounded border px-3 py-1 text-sm hover:bg-gray-100"
        >
          キャンセル
        </button>
        <button
          onClick={() => void handleConfirm()}
          disabled={submitting}
          className="rounded bg-red-600 px-3 py-1 text-sm text-white hover:bg-red-700 disabled:opacity-40"
        >
          削除
        </button>
      </div>
    </Modal>
  );
}
