import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Mail } from "../../types/mail";
import { attachmentApi } from "../../api/attachmentApi";
import { remoteImageApi } from "../../api/remoteImageApi";
import { errorMessage } from "../../api/errors";
import { useErrorStore } from "../../stores/errorStore";
import { hasCidReferences, replaceCidReferences } from "../../utils/inlineImages";
import {
  extractExternalImageUrls,
  replaceExternalImageUrls,
  type FetchedExternalImage,
} from "../../utils/externalImages";
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
  // 外部画像はユーザーの明示操作でのみ取得する（C9）。永続化はしない:
  // メールを開き直すと再び遮断状態に戻る（設計書 2026-07-15-external-image-optin-design.md）
  const [externalImages, setExternalImages] = useState<FetchedExternalImage[] | null>(null);
  const [loadingImages, setLoadingImages] = useState(false);

  useEffect(() => {
    setExternalImages(null);
  }, [mail.id]);

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

  const externalUrls = useMemo(
    () => (resolvedHtml ? extractExternalImageUrls(resolvedHtml) : []),
    [resolvedHtml],
  );

  const displayHtml = useMemo(() => {
    if (!resolvedHtml || !externalImages) return resolvedHtml;
    return replaceExternalImageUrls(resolvedHtml, externalImages);
  }, [resolvedHtml, externalImages]);

  const showExternalImages = async () => {
    setLoadingImages(true);
    try {
      setExternalImages(await remoteImageApi.fetchExternalImages(externalUrls));
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    } finally {
      setLoadingImages(false);
    }
  };

  const wireFrame = useCallback(() => {
    if (frameRef.current) {
      wireFrameDocument(frameRef.current, setFrameHeight);
    }
  }, []);

  // WKWebView では srcdoc ロード完了の onLoad で配線される。jsdom（テスト）は
  // srcdoc をロードしないため、マウント直後の about:blank 文書にも配線しておく
  useEffect(() => {
    wireFrame();
  }, [wireFrame, displayHtml]);

  return (
    <div className="selectable flex-1 overflow-y-auto px-6 py-4">
      {externalUrls.length > 0 && externalImages === null && (
        <div className="mb-3 flex items-center justify-between gap-3 rounded border border-gray-200 bg-gray-50 px-3 py-2 text-xs text-gray-600">
          <span>
            外部画像 {externalUrls.length} 件をブロックしました（表示すると開封が送信者に通知されます）
          </span>
          <button
            type="button"
            onClick={() => void showExternalImages()}
            disabled={loadingImages}
            className="shrink-0 rounded border border-gray-300 bg-white px-2 py-1 text-xs hover:bg-gray-100 disabled:opacity-50"
          >
            {loadingImages ? "取得中…" : "画像を表示"}
          </button>
        </div>
      )}
      {displayHtml ? (
        <iframe
          ref={frameRef}
          title="メール本文"
          // 本文HTMLはアプリ本体と別の閉じた文書に隔離する（DOMPurifyバイパス時に
          // Tauri IPCへ到達させない第3層防御）。allow-scripts は決して付けない
          sandbox="allow-same-origin"
          srcDoc={buildMailFrameSrcdoc(sanitizeMailHtml(displayHtml))}
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
