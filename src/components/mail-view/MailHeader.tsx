import type { Mail } from "../../types/mail";
import { formatFullDate } from "../../utils/date";
import { ClassifyResultBadge } from "../common/ClassifyResultBadge";

interface MailHeaderProps {
  mail: Mail;
  /** 渡すとバッジがクリック可能になり、確認バーを開く導線になる */
  onBadgeClick?: () => void;
}

export function MailHeader({ mail, onBadgeClick }: MailHeaderProps) {
  return (
    <div className="selectable border-b px-6 py-4">
      <div className="flex items-center gap-2">
        <h2 className="text-lg font-semibold">{mail.subject}</h2>
        {mail.assigned_by != null && mail.confidence != null && (
          <ClassifyResultBadge
            confidence={mail.confidence}
            assignedBy={mail.assigned_by}
            onClick={onBadgeClick}
          />
        )}
      </div>
      <div className="mt-2 space-y-1 text-sm text-gray-600">
        <div>
          <span className="font-medium">From:</span> {mail.from_addr}
        </div>
        <div>
          <span className="font-medium">To:</span> {mail.to_addr}
        </div>
        {mail.cc_addr && (
          <div>
            <span className="font-medium">Cc:</span> {mail.cc_addr}
          </div>
        )}
        <div>
          <span className="font-medium">Date:</span>{" "}
          {formatFullDate(mail.date)}
        </div>
      </div>
    </div>
  );
}
