import { useRef } from "react";
import type { Thread } from "../../types/mail";
import { useDragStore } from "../../stores/dragStore";

interface ThreadItemProps {
  thread: Thread;
  selected: boolean;
  onClick: () => void;
}

export function ThreadItem({ thread, selected, onClick }: ThreadItemProps) {
  const date = new Date(thread.last_date);
  const dateStr = `${date.getMonth() + 1}/${date.getDate()}`;
  const startDrag = useDragStore((s) => s.startDrag);
  const updatePosition = useDragStore((s) => s.updatePosition);
  const isDragging = useRef(false);
  const startPos = useRef({ x: 0, y: 0 });

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    startPos.current = { x: e.clientX, y: e.clientY };
    isDragging.current = false;

    const handleMouseMove = (me: MouseEvent) => {
      const dx = me.clientX - startPos.current.x;
      const dy = me.clientY - startPos.current.y;
      if (!isDragging.current && Math.abs(dx) + Math.abs(dy) > 5) {
        isDragging.current = true;
        const mailIds = thread.mails.map((m) => m.id);
        startDrag(mailIds, thread.subject);
        updatePosition(me.clientX, me.clientY);
      }
      if (isDragging.current) {
        updatePosition(me.clientX, me.clientY);
      }
    };

    const handleMouseUp = () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      if (!isDragging.current) {
        onClick();
      }
      isDragging.current = false;
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  return (
    <div
      onMouseDown={handleMouseDown}
      className={`w-full cursor-pointer border-b px-4 py-3 text-left hover:bg-gray-50 ${selected ? "bg-blue-50" : ""}`}
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
    </div>
  );
}
