import type { Mail } from "../../types/mail";
import { useComposeStore } from "../../stores/composeStore";
import { useMailStore } from "../../stores/mailStore";

interface MailActionsProps {
  mail: Mail;
}

export function MailActions({ mail }: MailActionsProps) {
  const openCompose = useComposeStore((s) => s.openCompose);
  const archiveMail = useMailStore((s) => s.archiveMail);
  const unarchiveMail = useMailStore((s) => s.unarchiveMail);
  const deleteMail = useMailStore((s) => s.deleteMail);
  const toggleFlagged = useMailStore((s) => s.toggleFlagged);
  const markMailUnread = useMailStore((s) => s.markMailUnread);

  const isArchived = mail.folder === "Archive";

  const buttonClass =
    "rounded border px-3 py-1 text-sm text-gray-700 hover:bg-gray-100";
  const deleteButtonClass =
    "rounded border px-3 py-1 text-sm text-red-600 hover:bg-red-50";
  const starButtonClass =
    "rounded border px-3 py-1 text-sm text-amber-500 hover:bg-amber-50";

  const handleDelete = () => {
    // 削除はサーバーからも消える破壊的操作のため必ず確認する
    if (
      window.confirm(
        "このメールを削除しますか？サーバーにゴミ箱があればゴミ箱へ移動し、無い場合は完全に削除されます。",
      )
    ) {
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
      {isArchived ? (
        <button
          className={buttonClass}
          onClick={() => void unarchiveMail(mail)}
        >
          アーカイブ解除
        </button>
      ) : (
        <button className={buttonClass} onClick={() => void archiveMail(mail)}>
          アーカイブ
        </button>
      )}
      <button className={deleteButtonClass} onClick={handleDelete}>
        削除
      </button>
      <button
        className={starButtonClass}
        onClick={() => void toggleFlagged(mail)}
        aria-label={mail.is_flagged ? "★" : "☆"}
      >
        {mail.is_flagged ? "★" : "☆"}
      </button>
      <button className={buttonClass} onClick={() => void markMailUnread(mail)}>
        未読にする
      </button>
    </div>
  );
}
