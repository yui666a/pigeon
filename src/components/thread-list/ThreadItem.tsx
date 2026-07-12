import { memo } from "react";
import type { Thread } from "../../types/mail";
import { useMailDrag } from "../../hooks/useMailDrag";
import { formatShortDate } from "../../utils/date";
import { useSelectionStore } from "../../stores/selectionStore";

interface ThreadItemProps {
  thread: Thread;
  selected: boolean;
  onClick: () => void;
}

export const ThreadItem = memo(function ThreadItem({
  thread,
  selected,
  onClick,
}: ThreadItemProps) {
  const dateStr = formatShortDate(thread.last_date);
  const mailIds = thread.mails.map((m) => m.id);
  const { onMouseDown } = useMailDrag(mailIds, thread.subject, onClick);
  const hasUnread = thread.mails.some((m) => !m.is_read);
  const isChecked = useSelectionStore((s) => s.isSelected(thread.thread_id));
  const toggleThread = useSelectionStore((s) => s.toggleThread);

  return (
    <div
      onMouseDown={onMouseDown}
      className={`flex w-full cursor-pointer items-start gap-2 border-b px-4 py-3 text-left hover:bg-gray-50 ${selected ? "bg-blue-50" : ""}`}
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
          <span
            className={`truncate text-sm ${hasUnread ? "font-bold text-gray-900" : "font-medium"}`}
          >
            {thread.subject}
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
