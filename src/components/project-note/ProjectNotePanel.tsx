import { useEffect, useState } from "react";
import { useProjectNoteStore } from "../../stores/projectNoteStore";
import { ProjectNoteEditor } from "./ProjectNoteEditor";
import { Modal } from "../common/Modal";

interface ProjectNotePanelProps {
  projectId: string;
}

type Tab = "note" | "ai";

/**
 * 案件ノートのパネル。「ノート」と「AI要約」をタブで切り替える。
 * AI要約もユーザーが編集でき、再生成時は手修正があれば確認ダイアログを出す
 * （設計書 2026-07-19-project-notes-design.md §3-5）。
 * エラーはトースト（グローバルの errorStore）経由で表示され、本コンポーネントは関与しない。
 */
export function ProjectNotePanel({ projectId }: ProjectNotePanelProps) {
  const {
    note,
    history,
    generating,
    load,
    saveUser,
    saveAi,
    generate,
    loadHistory,
    restore,
  } = useProjectNoteStore();

  const [tab, setTab] = useState<Tab>("note");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  useEffect(() => {
    void load(projectId);
  }, [projectId, load]);

  const onRegenerate = () => {
    if (note?.ai_edited) {
      setConfirmOpen(true);
      return;
    }
    void generate(projectId);
  };

  const confirmRegenerate = () => {
    setConfirmOpen(false);
    void generate(projectId);
  };

  const openHistory = () => {
    setHistoryOpen(true);
    void loadHistory(projectId);
  };

  const tabCls = (active: boolean) =>
    `px-3 py-1 text-sm ${active ? "border-b-2 border-blue-500 font-semibold" : ""}`;

  return (
    <div className="flex flex-col gap-2 p-2">
      <div role="tablist" className="flex border-b">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "note"}
          className={tabCls(tab === "note")}
          onClick={() => setTab("note")}
        >
          ノート
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "ai"}
          className={tabCls(tab === "ai")}
          onClick={() => setTab("ai")}
        >
          AI要約
        </button>
      </div>

      {tab === "note" && (
        <ProjectNoteEditor
          value={note?.user_md ?? ""}
          onChange={(md) => void saveUser(projectId, md)}
          ariaLabel="案件ノート"
        />
      )}

      {tab === "ai" && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onRegenerate}
              disabled={generating}
              className="rounded border px-2 py-1 text-sm disabled:opacity-50"
            >
              {generating ? "生成中…" : note?.ai_md ? "再生成" : "生成"}
            </button>
            <button
              type="button"
              onClick={openHistory}
              className="rounded border px-2 py-1 text-sm"
            >
              履歴
            </button>
            {note?.ai_generated_at && (
              <span className="text-xs text-gray-500">
                最終生成: {note.ai_generated_at}
              </span>
            )}
          </div>

          <ProjectNoteEditor
            value={note?.ai_md ?? ""}
            onChange={(md) => void saveAi(projectId, md)}
            ariaLabel="AI要約"
          />
        </div>
      )}

      {confirmOpen && (
        <Modal ariaLabel="再生成の確認" onClose={() => setConfirmOpen(false)} className="w-80 p-4">
          <p className="text-sm">
            AI要約に手動の修正があります。再生成すると上書きされます（元の内容は履歴から戻せます）。
          </p>
          <div className="mt-3 flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setConfirmOpen(false)}
              className="rounded border px-3 py-1 text-sm hover:bg-gray-100"
            >
              キャンセル
            </button>
            <button
              type="button"
              onClick={confirmRegenerate}
              className="rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700"
            >
              上書きする
            </button>
          </div>
        </Modal>
      )}

      {historyOpen && (
        <div className="rounded border p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold">AI要約の履歴</span>
            <button
              type="button"
              onClick={() => setHistoryOpen(false)}
              className="text-sm"
              aria-label="履歴を閉じる"
            >
              ×
            </button>
          </div>
          {history.length === 0 ? (
            <p className="text-sm text-gray-500">履歴はありません</p>
          ) : (
            <ul className="mt-2 flex flex-col gap-2">
              {history.map((h) => (
                <li key={h.id} className="rounded border p-2">
                  <div className="text-xs text-gray-500">{h.replaced_at}</div>
                  <pre className="whitespace-pre-wrap text-xs">{h.ai_md}</pre>
                  <button
                    type="button"
                    onClick={() => void restore(projectId, h.id)}
                    className="mt-1 rounded border px-2 py-0.5 text-xs"
                  >
                    この版に戻す
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
