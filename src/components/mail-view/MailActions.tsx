import type { Mail } from "../../types/mail";
import { useComposeStore } from "../../stores/composeStore";
import { useMailStore } from "../../stores/mailStore";

interface MailActionsProps {
  mail: Mail;
}

export function MailActions({ mail }: MailActionsProps) {
  const openCompose = useComposeStore((s) => s.openCompose);
  const archiveMail = useMailStore((s) => s.archiveMail);
  const deleteMail = useMailStore((s) => s.deleteMail);

  const buttonClass =
    "rounded border px-3 py-1 text-sm text-gray-700 hover:bg-gray-100";
  const deleteButtonClass =
    "rounded border px-3 py-1 text-sm text-red-600 hover:bg-red-50";

  const handleDelete = () => {
    // 削除はサーバーからも消える破壊的操作のため必ず確認する
    if (window.confirm("このメールを削除しますか？この操作は取り消せません。")) {
      void deleteMail(mail);
    }
  };

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
      <button className={buttonClass} onClick={() => void archiveMail(mail)}>
        アーカイブ
      </button>
      <button className={deleteButtonClass} onClick={handleDelete}>
        削除
      </button>
    </div>
  );
}
