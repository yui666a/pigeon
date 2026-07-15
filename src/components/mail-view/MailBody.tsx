import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Mail } from "../../types/mail";
import { attachmentApi } from "../../api/attachmentApi";
import { hasCidReferences, replaceCidReferences } from "../../utils/inlineImages";
import { sanitizeMailHtml } from "../../utils/sanitizeMailHtml";
import { buildMailFrameSrcdoc } from "../../utils/buildMailFrameSrcdoc";
import { resolveOpenableUrl } from "../../utils/mailLinkPolicy";
import { AttachmentList } from "./AttachmentList";

interface MailBodyProps {
  mail: Mail;
}

/** クリックリスナー等を配線済みの iframe 文書（srcdoc 再ロードごとに新しい文書になる） */
const wiredDocs = new WeakSet<Document>();

/**
 * 隔離 iframe の文書に本文リンクの捕捉と高さ同期を配線する。
 * sandbox（allow-scripts なし）でスクリプトは実行されないため、
 * 親側からリスナーを張る。文書単位で冪等。
 */
function wireFrameDocument(
  frame: HTMLIFrameElement,
  onHeight: (height: number) => void,
): void {
  const doc = frame.contentDocument;
  if (!doc || wiredDocs.has(doc)) return;
  wiredDocs.add(doc);

  doc.addEventListener("click", (e) => {
    const anchor = (e.target as Element | null)?.closest?.("a");
    if (!anchor) return;
    e.preventDefault();
    const url = resolveOpenableUrl(anchor.getAttribute("href"));
    if (url) void openUrl(url);
  });

  const syncHeight = () => {
    const height = doc.body?.scrollHeight ?? 0;
    if (height > 0) onHeight(height);
  };
  syncHeight();
  // 画像の遅延ロード等で本文の高さが変わったら追従する
  if (typeof ResizeObserver !== "undefined" && doc.body) {
    new ResizeObserver(syncHeight).observe(doc.body);
  }
}

export function MailBody({ mail }: MailBodyProps) {
  const bodyHtml = mail.body_html;
  const [resolvedHtml, setResolvedHtml] = useState(bodyHtml);
  const [frameHeight, setFrameHeight] = useState(0);
  const frameRef = useRef<HTMLIFrameElement>(null);

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

  const wireFrame = useCallback(() => {
    if (frameRef.current) {
      wireFrameDocument(frameRef.current, setFrameHeight);
    }
  }, []);

  // WKWebView では srcdoc ロード完了の onLoad で配線される。jsdom（テスト）は
  // srcdoc をロードしないため、マウント直後の about:blank 文書にも配線しておく
  useEffect(() => {
    wireFrame();
  }, [wireFrame, resolvedHtml]);

  return (
    <div className="selectable flex-1 overflow-y-auto px-6 py-4">
      {resolvedHtml ? (
        <iframe
          ref={frameRef}
          title="メール本文"
          // 本文HTMLはアプリ本体と別の閉じた文書に隔離する（DOMPurifyバイパス時に
          // Tauri IPCへ到達させない第3層防御）。allow-scripts は決して付けない
          sandbox="allow-same-origin"
          srcDoc={buildMailFrameSrcdoc(sanitizeMailHtml(resolvedHtml))}
          onLoad={wireFrame}
          className="w-full border-0"
          style={{ height: frameHeight > 0 ? `${frameHeight}px` : "60vh" }}
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
