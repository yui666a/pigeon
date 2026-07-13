import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Attachment } from "../../types/attachment";
import { useErrorStore } from "../../stores/errorStore";

interface AttachmentListProps {
  mailId: string;
}

function formatSize(size: number | null): string {
  if (size === null) return "";
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

export function AttachmentList({ mailId }: AttachmentListProps) {
  const [attachments, setAttachments] = useState<Attachment[] | null>(null);
  const [loading, setLoading] = useState(false);

  const loadAttachments = async () => {
    setLoading(true);
    try {
      const result = await invoke<Attachment[]>("list_attachments", { mailId });
      setAttachments(result);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const saveAttachment = async (attachment: Attachment) => {
    try {
      // 保存先の選択はバックエンドがネイティブダイアログで行う
      // （IPC 経由で保存先パスを渡さない。キャンセル時は false が返る）
      await invoke<boolean>("save_attachment", {
        attachmentId: attachment.id,
      });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  if (attachments === null) {
    return (
      <div className="mt-4 border-t border-gray-200 pt-3">
        <button
          onClick={() => void loadAttachments()}
          disabled={loading}
          className="rounded border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50 disabled:opacity-50"
        >
          {loading ? "添付ファイルを取得中..." : "📎 添付ファイルを表示"}
        </button>
      </div>
    );
  }

  return (
    <div className="mt-4 border-t border-gray-200 pt-3">
      <p className="mb-2 text-xs font-medium text-gray-500">
        添付ファイル ({attachments.length})
      </p>
      {attachments.length === 0 ? (
        <p className="text-sm text-gray-400">添付ファイルはありません</p>
      ) : (
        <ul className="space-y-1">
          {attachments.map((attachment) => (
            <li
              key={attachment.id}
              className="flex items-center gap-3 rounded border border-gray-200 px-3 py-1.5 text-sm"
            >
              <span className="min-w-0 flex-1 truncate" title={attachment.filename}>
                {attachment.filename}
              </span>
              <span className="shrink-0 text-xs text-gray-400">
                {formatSize(attachment.size)}
              </span>
              <button
                onClick={() => void saveAttachment(attachment)}
                className="shrink-0 rounded bg-blue-600 px-2 py-0.5 text-xs text-white hover:bg-blue-700"
              >
                保存
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
