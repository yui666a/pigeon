import type { Mail } from "../../types/mail";
import { useComposeStore } from "../../stores/composeStore";

interface MailActionsProps {
  mail: Mail;
}

export function MailActions({ mail }: MailActionsProps) {
  const openCompose = useComposeStore((s) => s.openCompose);

  const buttonClass =
    "rounded border px-3 py-1 text-sm text-gray-700 hover:bg-gray-100";

  return (
    <div className="flex gap-2 border-b px-6 py-2">
      <button className={buttonClass} onClick={() => openCompose("reply", mail)}>
        返信
      </button>
      <button
        className={buttonClass}
        onClick={() => openCompose("replyAll", mail)}
      >
        全員に返信
      </button>
      <button
        className={buttonClass}
        onClick={() => openCompose("forward", mail)}
      >
        転送
      </button>
    </div>
  );
}
