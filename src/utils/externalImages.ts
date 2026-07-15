/** 外部画像の取得結果（Rust の fetch_external_images が返す） */
export interface FetchedExternalImage {
  url: string;
  data_uri: string;
}

/** 1メールあたりの外部画像の取得上限（資源枯渇防止。バックエンドと同値） */
export const MAX_EXTERNAL_IMAGES = 20;

/**
 * HTML本文中の外部画像URL（http/https の img src）を重複除去して列挙する。
 * 「画像を表示」ボタンの表示判定と、fetch_external_images への入力に使う。
 * プロトコル相対URL（//...）と相対URLは対象外（サニタイザで除去されたまま）。
 */
export function extractExternalImageUrls(html: string): string[] {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const urls = new Set<string>();
  for (const img of doc.querySelectorAll("img[src]")) {
    const src = img.getAttribute("src") ?? "";
    if (/^https?:\/\//i.test(src)) {
      urls.add(src);
      if (urls.size >= MAX_EXTERNAL_IMAGES) break;
    }
  }
  return [...urls];
}

/**
 * 取得済みの外部画像URLを data URI に置換する（cid置換 replaceCidReferences と同型）。
 * 取得できなかったURLはそのまま残し、サニタイザの除去に委ねる。
 *
 * 正規表現によるHTML書き換えは属性値のエスケープを崩す恐れがあるため、
 * DOMParser でパースして該当 img だけを差し替える。
 */
export function replaceExternalImageUrls(
  html: string,
  images: FetchedExternalImage[],
): string {
  // バックエンドも Content-Type を許可リストで検証するが、フロントでも画像以外の
  // data URI（data:text/html 等）を差し込まない（防御の多層化）
  const byUrl = new Map(
    images
      .filter((img) => img.data_uri.startsWith("data:image/"))
      .map((img) => [img.url, img.data_uri]),
  );
  if (byUrl.size === 0) return html;

  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.querySelectorAll("img[src]").forEach((img) => {
    const dataUri = byUrl.get(img.getAttribute("src") ?? "");
    if (dataUri) {
      img.setAttribute("src", dataUri);
    }
  });
  return doc.body.innerHTML;
}
