import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Mail } from "../../types/mail";
import { attachmentApi } from "../../api/attachmentApi";
import { hasCidReferences, replaceCidReferences } from "../../utils/inlineImages";
import { sanitizeMailHtml } from "../../utils/sanitizeMailHtml";
import { AttachmentList } from "./AttachmentList";

interface MailBodyProps {
  mail: Mail;
}

/** メール本文由来のリンクで Webview を遷移させないための許可スキーム */
const ALLOWED_LINK_PROTOCOLS = ["http:", "https:", "mailto:"];

/**
 * 本文内リンクのクリックを捕捉し、http(s)/mailto のみ外部ブラウザで開く。
 * アドレスバーの無いネイティブ窓が本文起因でフィッシングサイトへ遷移するのを防ぐ。
 * カスタムスキーム（自アプリの deep-link を含む）と相対URLは開かない。
 */
function handleBodyLinkClick(e: React.MouseEvent<HTMLDivElement>) {
  const anchor = (e.target as Element | null)?.closest?.("a");
  if (!anchor) return;
  e.preventDefault();
  const href = anchor.getAttribute("href");
  if (!href) return;
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return;
  }
  if (ALLOWED_LINK_PROTOCOLS.includes(url.protocol)) {
    void openUrl(href);
  }
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
        const images = await attachmentApi.fetchInlineImages(mail.id);
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
    <div className="selectable flex-1 overflow-y-auto px-6 py-4">
      {resolvedHtml ? (
        <div
          className="prose max-w-none text-sm"
          onClickCapture={handleBodyLinkClick}
          dangerouslySetInnerHTML={{ __html: sanitizeMailHtml(resolvedHtml) }}
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
