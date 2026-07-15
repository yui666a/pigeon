import DOMPurify from "dompurify";

/**
 * メール本文HTML専用のサニタイザ。
 *
 * メール本文は最も敵対的な入力（外部から届く悪意あるHTML）であるため、
 * DOMPurify デフォルトより厳しい設定で描画専用のタグ/属性だけを通す。
 *
 * - form系（form/input/button/textarea/select）: フィッシングUIと外部POSTの排除
 * - style要素/style属性: 全画面オーバーレイ等のUIリドレッシングとCSS経由の漏洩排除
 * - iframe/object/embed: 埋め込みコンテンツの排除
 * - a要素: rel="noopener noreferrer" を強制付与し target を除去
 *   （クリック時の遷移制御は MailBody 側の onClick が担う）
 * - img要素: src は data:image/ と cid: のみ許可し、外部 http(s) 画像は除去
 *   （トラッキングピクセル・IPリーク対策。CSP img-src が第一防御だが
 *   単独依存にしない）。srcset は外部URLの迂回経路になるため常に除去
 *
 * グローバルの DOMPurify にフックを足すと他のサニタイズ呼び出し
 * （SearchResults 等）に影響するため、専用インスタンスを使う。
 */
const purifier = DOMPurify(window);

/** メール本文の img src として許可するプレフィックス */
const ALLOWED_IMG_SRC_PREFIXES = ["data:image/", "cid:"];

purifier.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("rel", "noopener noreferrer");
    node.removeAttribute("target");
  }
  if (node.tagName === "IMG") {
    node.removeAttribute("srcset");
    const src = node.getAttribute("src") ?? "";
    if (!ALLOWED_IMG_SRC_PREFIXES.some((prefix) => src.startsWith(prefix))) {
      node.removeAttribute("src");
    }
  }
});

const MAIL_SANITIZE_CONFIG: import("dompurify").Config = {
  USE_PROFILES: { html: true },
  FORBID_TAGS: [
    "style",
    "form",
    "input",
    "button",
    "textarea",
    "select",
    "iframe",
    "object",
    "embed",
  ],
  FORBID_ATTR: ["style"],
};

export function sanitizeMailHtml(html: string): string {
  return purifier.sanitize(html, MAIL_SANITIZE_CONFIG);
}
