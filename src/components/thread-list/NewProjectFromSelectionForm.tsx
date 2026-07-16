import { useEffect, useState } from "react";
import { classifyApi } from "../../api/classifyApi";
import { useErrorStore } from "../../stores/errorStore";
import { errorMessage } from "../../api/errors";

interface NewProjectFromSelectionFormProps {
  /** 開いた時点で固定された選択メール ID（提案と作成の対象を一致させる） */
  mailIds: string[];
  /** 案件名・説明を確定したときに呼ぶ。作成→移動は呼び出し元が担う */
  onCreate: (name: string, description: string | undefined) => void;
  onCancel: () => void;
}

/**
 * 未分類の選択メールから新規案件を作る展開フォーム。
 * マウント時に LLM 提案を取得して初期値化し、名前・説明はユーザーが編集できる。
 * 提案取得に失敗しても空フォームで開き、手入力で作成できる
 * （設計書 2026-07-17-group-unclassified-into-new-project-design.md）。
 */
export function NewProjectFromSelectionForm({
  mailIds,
  onCreate,
  onCancel,
}: NewProjectFromSelectionFormProps) {
  const [loading, setLoading] = useState(true);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const addError = useErrorStore((s) => s.addError);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const s = await classifyApi.suggestProjectFromMails(mailIds);
        if (cancelled) return;
        setName(s.name);
        setDescription(s.description);
      } catch (e) {
        if (!cancelled) addError(errorMessage(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [mailIds, addError]);

  return (
    <div className="border-b bg-gray-50 px-4 py-3">
      <p className="mb-2 text-xs font-medium text-gray-600">
        選択した {mailIds.length} 件で新しい案件を作成
      </p>
      {loading ? (
        <p className="text-xs text-gray-500">案件名を提案中…</p>
      ) : (
        <div className="space-y-2">
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="案件名を入力"
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
              onClick={() => onCreate(name.trim(), description.trim() || undefined)}
              disabled={!name.trim()}
              className="rounded bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700 disabled:opacity-50"
            >
              作成（{mailIds.length}件を移動）
            </button>
            <button
              onClick={onCancel}
              className="rounded border border-gray-300 px-3 py-1 text-xs text-gray-600 hover:bg-gray-100"
            >
              キャンセル
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
