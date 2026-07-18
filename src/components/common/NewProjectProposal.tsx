import { useState } from "react";
import type { Project } from "../../types/project";
import { projectPathString } from "../../stores/projectTree";

interface NewProjectProposalProps {
  mailId: string;
  suggestedName: string;
  suggestedDescription?: string;
  reason: string;
  /** AI提案の作成先（子案件として作成する場合の親）。ルート作成の提案なら undefined */
  parentProjectId?: string;
  /** 親選択ドロップダウンの候補（ルートを含む案件一覧） */
  projects: Project[];
  onApprove: (
    mailId: string,
    name: string,
    description?: string,
    parentProjectId?: string,
  ) => void;
  onReject: (mailId: string) => void;
}

export function NewProjectProposal({
  mailId,
  suggestedName,
  suggestedDescription,
  reason,
  parentProjectId,
  projects,
  onApprove,
  onReject,
}: NewProjectProposalProps) {
  const [name, setName] = useState(suggestedName);
  const [description, setDescription] = useState(suggestedDescription ?? "");
  const [parentId, setParentId] = useState(parentProjectId ?? "");

  const targetPath = parentId ? projectPathString(projects, parentId) : "";
  const sortedProjects = [...projects].sort((a, b) =>
    projectPathString(projects, a.id).localeCompare(
      projectPathString(projects, b.id),
    ),
  );

  return (
    <div className="rounded border border-yellow-300 bg-yellow-50 p-3">
      <p className="mb-2 text-xs text-yellow-700">{reason}</p>
      <div className="space-y-2">
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="案件名"
          className="w-full rounded border border-gray-300 px-2 py-1 text-sm focus:border-blue-400 focus:outline-none"
        />
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="説明（任意）"
          className="w-full rounded border border-gray-300 px-2 py-1 text-sm focus:border-blue-400 focus:outline-none"
        />
        <label className="flex items-center gap-2 text-xs text-gray-600">
          作成先の親案件
          <select
            aria-label="作成先の親案件"
            value={parentId}
            onChange={(e) => setParentId(e.target.value)}
            className="min-w-0 flex-1 rounded border border-gray-300 px-2 py-1 text-sm"
          >
            <option value="">ルート（親なし）</option>
            {sortedProjects.map((p) => (
              <option key={p.id} value={p.id}>
                {projectPathString(projects, p.id)}
              </option>
            ))}
          </select>
        </label>
        {targetPath && (
          <p className="text-xs text-gray-600">作成先: {targetPath}</p>
        )}
        <div className="flex gap-2">
          <button
            onClick={() =>
              onApprove(mailId, name, description || undefined, parentId || undefined)
            }
            disabled={!name.trim()}
            className="rounded bg-blue-500 px-3 py-1 text-xs text-white hover:bg-blue-600 disabled:opacity-50"
          >
            案件を作成
          </button>
          <button
            onClick={() => onReject(mailId)}
            className="rounded bg-gray-200 px-3 py-1 text-xs text-gray-600 hover:bg-gray-300"
          >
            却下
          </button>
        </div>
      </div>
    </div>
  );
}
