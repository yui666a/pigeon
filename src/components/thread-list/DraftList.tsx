import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useComposeStore } from "../../stores/composeStore";
import { useDraftStore } from "../../stores/draftStore";
import { formatShortDate } from "../../utils/date";
import type { Draft } from "../../types/mail";

export function DraftList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const drafts = useDraftStore((s) => s.drafts);
  const fetchDrafts = useDraftStore((s) => s.fetchDrafts);
  const deleteDraft = useDraftStore((s) => s.deleteDraft);
  const openComposeFromDraft = useComposeStore((s) => s.openComposeFromDraft);

  useEffect(() => {
    if (selectedAccountId) {
      fetchDrafts(selectedAccountId);
    }
  }, [selectedAccountId, fetchDrafts]);

  if (!selectedAccountId) return null;

  const handleDelete = (e: React.MouseEvent, draft: Draft) => {
    e.stopPropagation();
    void deleteDraft(draft.id);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="border-b px-4 py-2">
        <h3 className="text-sm font-medium text-gray-700">
          下書き ({drafts.length})
        </h3>
      </div>

      {drafts.length === 0 ? (
        <div className="px-4 py-6 text-center text-sm text-gray-400">
          下書きはありません
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">
          {drafts.map((draft) => (
            <div
              key={draft.id}
              onClick={() => openComposeFromDraft(draft)}
              className="w-full cursor-pointer border-b px-4 py-3 text-left hover:bg-gray-50"
            >
              <div className="flex items-center justify-between">
                <span className="truncate text-sm font-medium">
                  {draft.subject || "(件名なし)"}
                </span>
                <span className="ml-2 shrink-0 text-xs text-gray-400">
                  {formatShortDate(draft.updated_at)}
                </span>
              </div>
              <div className="mt-1 flex items-center justify-between gap-2">
                <span className="truncate text-xs text-gray-500">
                  {draft.to_addr || "(宛先未設定)"}
                </span>
                <button
                  onClick={(e) => handleDelete(e, draft)}
                  className="shrink-0 text-xs text-red-500 hover:underline"
                  aria-label="削除"
                >
                  削除
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
