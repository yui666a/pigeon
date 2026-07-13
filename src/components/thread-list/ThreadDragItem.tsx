import { memo } from "react";
import type { Thread } from "../../types/mail";
import { useMailDrag } from "../../hooks/useMailDrag";
import { formatShortDate } from "../../utils/date";
import { threadBackgroundClass } from "../../utils/threadStyle";
import { useSelectionStore } from "../../stores/selectionStore";

interface ThreadDragItemProps {
  thread: Thread;
  onClick: () => void;
}

/**
 * 未分類一覧のスレッド行。ドラッグするとスレッド内の全メールIDが
 * 対象になる（案件へのD&D分類はスレッド単位で行われる）。
 *
 * 表示（日付・参加者・フラグ★・件数バッジ・既読/未読の背景）は通常一覧の
 * ThreadItem と揃える。ドラッグ挙動とコンパクトなレイアウト（py-2/border-t）は
 * 未分類一覧固有のため維持する。
 */
export const ThreadDragItem = memo(function ThreadDragItem({
  thread,
  onClick,
}: ThreadDragItemProps) {
  const { onMouseDown } = useMailDrag(
    thread.mails.map((m) => m.id),
    thread.subject,
    onClick,
  );
  const dateStr = formatShortDate(thread.last_date);
  const hasUnread = thread.mails.some((m) => !m.is_read);
  const hasFlagged = thread.mails.some((m) => m.is_flagged);
  const isChecked = useSelectionStore((s) => s.isSelected(thread.thread_id));
  const toggleThread = useSelectionStore((s) => s.toggleThread);
  const bgClass = threadBackgroundClass(thread);

  return (
    <div
      onMouseDown={onMouseDown}
      className={`flex w-full cursor-pointer items-start gap-2 border-t px-4 py-2 text-left hover:bg-gray-50 ${bgClass}`}
    >
      <input
        type="checkbox"
        aria-label="スレッドを選択"
        checked={isChecked}
        onClick={(e) => e.stopPropagation()}
        onMouseDown={(e) => e.stopPropagation()}
        onChange={() => toggleThread(thread)}
        className="mt-1 shrink-0"
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between">
          <span className="flex min-w-0 items-center gap-1">
            {hasFlagged && (
              <span className="shrink-0 text-amber-500" aria-label="フラグ付き">
                ★
              </span>
            )}
            <span
              className={`truncate text-sm ${hasUnread ? "font-bold text-gray-900" : "font-medium"}`}
            >
              {thread.subject}
            </span>
          </span>
          <span className="ml-2 shrink-0 text-xs text-gray-400">{dateStr}</span>
        </div>
        <div className="mt-1 flex items-center justify-between">
          <span className="truncate text-xs text-gray-500">
            {thread.from_addrs.join(", ")}
          </span>
          {thread.mail_count > 1 && (
            <span className="ml-2 shrink-0 rounded-full bg-gray-200 px-1.5 text-xs">
              {thread.mail_count}
            </span>
          )}
        </div>
      </div>
    </div>
  );
});
