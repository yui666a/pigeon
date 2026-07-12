import { useEffect, useState } from "react";
import DOMPurify from "dompurify";
import { invoke } from "@tauri-apps/api/core";
import type { Mail } from "../../types/mail";
import type { InlineImage } from "../../types/attachment";
import { hasCidReferences, replaceCidReferences } from "../../utils/inlineImages";
import { AttachmentList } from "./AttachmentList";

interface MailBodyProps {
  mail: Mail;
}

export function MailBody({ mail }: MailBodyProps) {
  const bodyHtml = mail.body_html;
  const [resolvedHtml, setResolvedHtml] = useState(bodyHtml);

  useEffect(() => {
    setResolvedHtml(bodyHtml);
    if (!bodyHtml || !hasCidReferences(bodyHtml) || !mail.has_attachments) return;

    let cancelled = false;
    void (async () => {
      try {
        const images = await invoke<InlineImage[]>("get_inline_images", { mailId: mail.id });
        if (!cancelled && images.length > 0) {
          setResolvedHtml(replaceCidReferences(bodyHtml, images));
        }
      } catch {
        // 取得失敗時は cid未解決のまま表示（壊れた画像アイコンになるだけ）
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [mail.id, mail.has_attachments, bodyHtml]);

  return (
    <div className="flex-1 overflow-y-auto px-6 py-4">
      {resolvedHtml ? (
        <div
          className="prose max-w-none text-sm"
          dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(resolvedHtml) }}
        />
      ) : (
        <pre className="whitespace-pre-wrap text-sm">{mail.body_text}</pre>
      )}
      {mail.has_attachments && (
        <AttachmentList key={mail.id} mailId={mail.id} />
      )}
    </div>
  );
}
