import { useEffect } from "react";
import { useComposeStore } from "../../stores/composeStore";
import { useAccountStore } from "../../stores/accountStore";

const MODE_TITLES = {
  new: "新規作成",
  reply: "返信",
  replyAll: "全員に返信",
  forward: "転送",
} as const;

export function ComposeModal() {
  const {
    isOpen,
    mode,
    to,
    cc,
    bcc,
    subject,
    body,
    sending,
    setField,
    send,
    closeCompose,
  } = useComposeStore();
  const hasAccount = useAccountStore(
    (s) => s.selectedAccountId !== null,
  );

  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // 送信中は閉じない（入力内容と送信状態を保護する）
      if (useComposeStore.getState().sending) return;
      void useComposeStore.getState().closeCompose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30">
      <div className="flex max-h-[85vh] w-[40rem] flex-col rounded-lg bg-white shadow-xl">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <h2 className="text-sm font-semibold">{MODE_TITLES[mode]}</h2>
          <button
            onClick={() => void closeCompose()}
            disabled={sending}
            className="text-gray-400 hover:text-gray-600 disabled:opacity-40"
            aria-label="閉じる"
          >
            ✕
          </button>
        </div>

        <div className="flex flex-1 flex-col gap-2 overflow-y-auto p-4">
          <label className="flex items-center gap-2 text-sm">
            <span className="w-10 shrink-0 text-gray-500">To</span>
            <input
              type="text"
              value={to}
              onChange={(e) => setField("to", e.target.value)}
              placeholder="宛先（カンマ区切り）"
              className="flex-1 rounded border px-2 py-1"
            />
          </label>
          <label className="flex items-center gap-2 text-sm">
            <span className="w-10 shrink-0 text-gray-500">Cc</span>
            <input
              type="text"
              value={cc}
              onChange={(e) => setField("cc", e.target.value)}
              className="flex-1 rounded border px-2 py-1"
            />
          </label>
          <label className="flex items-center gap-2 text-sm">
            <span className="w-10 shrink-0 text-gray-500">Bcc</span>
            <input
              type="text"
              value={bcc}
              onChange={(e) => setField("bcc", e.target.value)}
              className="flex-1 rounded border px-2 py-1"
            />
          </label>
          <label className="flex items-center gap-2 text-sm">
            <span className="w-10 shrink-0 text-gray-500">件名</span>
            <input
              type="text"
              value={subject}
              onChange={(e) => setField("subject", e.target.value)}
              className="flex-1 rounded border px-2 py-1"
            />
          </label>
          <label className="flex flex-1 flex-col gap-1 text-sm">
            <span className="sr-only">本文</span>
            <textarea
              value={body}
              onChange={(e) => setField("body", e.target.value)}
              rows={12}
              className="w-full flex-1 resize-none rounded border px-2 py-1 font-mono text-sm"
            />
          </label>
        </div>

        <div className="flex items-center justify-end gap-2 border-t px-4 py-3">
          <button
            onClick={() => void closeCompose()}
            disabled={sending}
            className="rounded border px-4 py-1.5 text-sm hover:bg-gray-100 disabled:opacity-40"
          >
            キャンセル
          </button>
          <button
            onClick={() => void send()}
            disabled={sending || !hasAccount}
            className="flex items-center gap-2 rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-40"
          >
            {sending && (
              <span
                role="status"
                className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-white border-t-transparent"
              />
            )}
            {sending ? "送信中" : "送信"}
          </button>
        </div>
      </div>
    </div>
  );
}
