import { memo } from "react";
import type { Thread } from "../../types/mail";
import { useMailDrag } from "../../hooks/useMailDrag";

interface ThreadDragItemProps {
  thread: Thread;
  onClick: () => void;
}

/**
 * 未分類一覧のスレッド行。ドラッグするとスレッド内の全メールIDが
 * 対象になる（案件へのD&D分類はスレッド単位で行われる）
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
  const hasUnread = thread.mails.some((m) => !m.is_read);
  const latestFrom = thread.mails[thread.mails.length - 1]?.from_addr ?? "";

  return (
    <div
      onMouseDown={onMouseDown}
      className="w-full cursor-pointer border-t px-4 py-2 text-left hover:bg-gray-50"
    >
      <div
        className={`truncate text-sm ${hasUnread ? "font-bold text-gray-900" : ""}`}
      >
        {thread.subject}
        {thread.mail_count > 1 && (
          <span className="ml-1 text-xs font-normal text-gray-400">
            ({thread.mail_count})
          </span>
        )}
      </div>
      <div className="truncate text-xs text-gray-500">{latestFrom}</div>
    </div>
  );
});
