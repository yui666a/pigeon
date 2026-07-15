import type { InlineImage } from "../types/attachment";

/**
 * HTML本文中の `<img src="cid:...">` を、対応する画像が見つかったものだけ
 * data URI に置換する。対応が見つからない cid参照はそのまま残す
 * （壊れた画像アイコンが出るだけで、外部リクエストは発生しない）。
 *
 * 正規表現によるHTML書き換えは属性値のエスケープを崩す恐れがあるため、
 * DOMParser でパースして img[src^="cid:"] だけを差し替える。
 */
export function replaceCidReferences(html: string, images: InlineImage[]): string {
  if (!html.includes("cid:")) return html;

  const byContentId = new Map(images.map((img) => [img.content_id, img.data_uri]));

  const doc = new DOMParser().parseFromString(html, "text/html");
  const imgs = doc.querySelectorAll('img[src^="cid:"]');
  imgs.forEach((img) => {
    const src = img.getAttribute("src") ?? "";
    const contentId = src.slice("cid:".length);
    const dataUri = byContentId.get(contentId);
    // バックエンドも MIME を許可リストで検証するが、フロントでも画像以外の
    // data URI（data:text/html 等）を差し込まない（防御の多層化）
    if (dataUri && dataUri.startsWith("data:image/")) {
      img.setAttribute("src", dataUri);
    }
  });

  return doc.body.innerHTML;
}

/** HTML本文が cid: 参照を含むか（get_inline_images の呼び出し要否の判定に使う） */
export function hasCidReferences(html: string): boolean {
  return /<img[^>]+src=["']cid:/i.test(html);
}
