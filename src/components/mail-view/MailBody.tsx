import DOMPurify from "dompurify";
import type { Mail } from "../../types/mail";

interface MailBodyProps {
  mail: Mail;
}

export function MailBody({ mail }: MailBodyProps) {
  return (
    <div className="flex-1 overflow-y-auto px-6 py-4">
      {mail.body_html ? (
        <div
          className="prose max-w-none text-sm"
          dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(mail.body_html) }}
        />
      ) : (
        <pre className="whitespace-pre-wrap text-sm">{mail.body_text}</pre>
      )}
    </div>
  );
}
