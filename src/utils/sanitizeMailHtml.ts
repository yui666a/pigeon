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
 *
 * グローバルの DOMPurify にフックを足すと他のサニタイズ呼び出し
 * （SearchResults 等）に影響するため、専用インスタンスを使う。
 */
const purifier = DOMPurify(window);

purifier.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("rel", "noopener noreferrer");
    node.removeAttribute("target");
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
