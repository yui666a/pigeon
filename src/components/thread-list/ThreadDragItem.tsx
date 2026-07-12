import { memo } from "react";
import type { Thread } from "../../types/mail";
import { useMailDrag } from "../../hooks/useMailDrag";
import { useSelectionStore } from "../../stores/selectionStore";

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
  const isChecked = useSelectionStore((s) => s.isSelected(thread.thread_id));
  const toggleThread = useSelectionStore((s) => s.toggleThread);

  return (
    <div
      onMouseDown={onMouseDown}
      className="flex w-full cursor-pointer items-start gap-2 border-t px-4 py-2 text-left hover:bg-gray-50"
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
    </div>
  );
});
