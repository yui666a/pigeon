import type { Thread } from "../../types/mail";

interface ThreadItemProps {
  thread: Thread;
  selected: boolean;
  onClick: () => void;
}

export function ThreadItem({ thread, selected, onClick }: ThreadItemProps) {
  const date = new Date(thread.last_date);
  const dateStr = `${date.getMonth() + 1}/${date.getDate()}`;

  const handleDragStart = (e: React.DragEvent) => {
    // Set all mail IDs in this thread as drag data
    const mailIds = thread.mails.map((m) => m.id);
    e.dataTransfer.setData("application/pigeon-mail-ids", JSON.stringify(mailIds));
    e.dataTransfer.effectAllowed = "move";
  };

  return (
    <button
      draggable
      onDragStart={handleDragStart}
      onClick={onClick}
      className={`w-full border-b px-4 py-3 text-left hover:bg-gray-50 ${selected ? "bg-blue-50" : ""}`}
    >
      <div className="flex items-center justify-between">
        <span className="truncate text-sm font-medium">{thread.subject}</span>
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
    </button>
  );
}
