import { memo } from "react";
import type { Mail } from "../../types/mail";
import { useMailDrag } from "../../hooks/useMailDrag";

interface MailDragItemProps {
  mail: Mail;
  onClick: () => void;
}

export const MailDragItem = memo(function MailDragItem({
  mail,
  onClick,
}: MailDragItemProps) {
  const { onMouseDown } = useMailDrag([mail.id], mail.subject, onClick);

  return (
    <div
      onMouseDown={onMouseDown}
      className="w-full cursor-pointer border-t px-4 py-2 text-left hover:bg-gray-50"
    >
      <div className="truncate text-sm">{mail.subject}</div>
      <div className="truncate text-xs text-gray-500">{mail.from_addr}</div>
    </div>
  );
});
