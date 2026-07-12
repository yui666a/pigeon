import { useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useComposeStore } from "../../stores/composeStore";
import { useAccountStore } from "../../stores/accountStore";
import { useErrorStore } from "../../stores/errorStore";
import { setDefaultComposeFormat } from "../../utils/composeFormat";
import { RichTextEditor } from "./RichTextEditor";

const MODE_TITLES = {
  new: "新規作成",
  reply: "返信",
  replyAll: "全員に返信",
  forward: "転送",
} as const;

/** 添付合計サイズの上限（Rust の MAX_TOTAL_ATTACHMENT_BYTES と一致・25MB） */
const MAX_TOTAL_ATTACHMENT_BYTES = 25 * 1024 * 1024;

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

export function ComposeModal() {
  const {
    isOpen,
    mode,
    to,
    cc,
    bcc,
    subject,
    body,
    format,
    attachments,
    sending,
    setField,
    setFormat,
    addAttachments,
    removeAttachment,
    send,
    closeCompose,
  } = useComposeStore();
  const hasAccount = useAccountStore((s) => s.selectedAccountId !== null);

  const totalSize = attachments.reduce((sum, a) => sum + a.size, 0);
  const overLimit = totalSize > MAX_TOTAL_ATTACHMENT_BYTES;
  const isRich = format === "rich";

  const pickAttachments = async () => {
    try {
      const selected = await open({ multiple: true });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      const files = await Promise.all(
        paths.map(async (path) => {
          const name = path.split(/[/\\]/).pop() ?? path;
          let size = 0;
          try {
            // サイズ取得は Rust の stat_file に委ねる（plugin-fs 非依存）
            size = await invoke<number>("stat_file", { path });
          } catch {
            size = 0;
          }
          return { path, name, size };
        }),
      );
      addAttachments(files);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

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

          {/* 本文形式の切替と既定化 */}
          <div className="flex items-center gap-3 text-sm">
            <div className="flex overflow-hidden rounded border" role="group" aria-label="本文形式">
              <button
                type="button"
                onClick={() => setFormat("plain")}
                aria-pressed={!isRich}
                className={`px-3 py-1 ${
                  !isRich ? "bg-blue-600 text-white" : "hover:bg-gray-100"
                }`}
              >
                プレーン
              </button>
              <button
                type="button"
                onClick={() => setFormat("rich")}
                aria-pressed={isRich}
                className={`px-3 py-1 ${
                  isRich ? "bg-blue-600 text-white" : "hover:bg-gray-100"
                }`}
              >
                リッチ
              </button>
            </div>
            <button
              type="button"
              onClick={() => setDefaultComposeFormat(format)}
              className="text-xs text-gray-500 underline hover:text-gray-700"
            >
              この形式を既定にする
            </button>
          </div>

          {/* 本文（リッチ⇔プレーンで入力コンポーネントを切替） */}
          {isRich ? (
            <RichTextEditor value={body} onChange={(html) => setField("body", html)} />
          ) : (
            <label className="flex flex-1 flex-col gap-1 text-sm">
              <span className="sr-only">本文</span>
              <textarea
                value={body}
                onChange={(e) => setField("body", e.target.value)}
                rows={12}
                className="w-full flex-1 resize-none rounded border px-2 py-1 font-mono text-sm"
              />
            </label>
          )}

          {/* 添付ファイル */}
          <div className="flex flex-col gap-1 text-sm">
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => void pickAttachments()}
                className="rounded border px-2 py-1 text-xs hover:bg-gray-100"
              >
                📎 添付を追加
              </button>
              {attachments.length > 0 && (
                <span
                  className={`text-xs ${overLimit ? "font-semibold text-red-600" : "text-gray-500"}`}
                >
                  合計 {formatSize(totalSize)}
                  {overLimit && `（上限 ${formatSize(MAX_TOTAL_ATTACHMENT_BYTES)} 超過）`}
                </span>
              )}
            </div>
            {attachments.length > 0 && (
              <ul className="flex flex-col gap-1">
                {attachments.map((a) => (
                  <li
                    key={a.path}
                    className="flex items-center justify-between rounded border px-2 py-1 text-xs"
                  >
                    <span className="truncate" title={a.path}>
                      {a.name}{" "}
                      <span className="text-gray-400">({formatSize(a.size)})</span>
                    </span>
                    <button
                      type="button"
                      onClick={() => removeAttachment(a.path)}
                      className="ml-2 shrink-0 text-gray-400 hover:text-gray-600"
                      aria-label={`${a.name} を削除`}
                    >
                      ✕
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
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
            disabled={sending || !hasAccount || overLimit}
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
