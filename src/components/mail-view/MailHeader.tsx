import type { Mail } from "../../types/mail";
import { formatFullDate } from "../../utils/date";

interface MailHeaderProps {
  mail: Mail;
}

export function MailHeader({ mail }: MailHeaderProps) {
  return (
    <div className="border-b px-6 py-4">
      <h2 className="text-lg font-semibold">{mail.subject}</h2>
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
