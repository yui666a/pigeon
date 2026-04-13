import { useState } from "react";

interface NewProjectProposalProps {
  mailId: string;
  suggestedName: string;
  suggestedDescription?: string;
  reason: string;
  onApprove: (mailId: string, name: string, description?: string) => void;
  onReject: (mailId: string) => void;
}

export function NewProjectProposal({
  mailId,
  suggestedName,
  suggestedDescription,
  reason,
  onApprove,
  onReject,
}: NewProjectProposalProps) {
  const [name, setName] = useState(suggestedName);
  const [description, setDescription] = useState(suggestedDescription ?? "");

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
        <div className="flex gap-2">
          <button
            onClick={() =>
              onApprove(mailId, name, description || undefined)
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
