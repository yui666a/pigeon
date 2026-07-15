/** メール本文由来のリンクで Webview を遷移させないための許可スキーム */
const ALLOWED_LINK_PROTOCOLS = ["http:", "https:", "mailto:"];

/**
 * メール本文内リンクの href を外部ブラウザで開いてよいか判定する。
 * アドレスバーの無いネイティブ窓が本文起因でフィッシングサイトへ遷移するのを防ぐ。
 * カスタムスキーム（自アプリの deep-link を含む）と相対URLは開かない。
 *
 * @returns 開いてよい場合は href そのもの、開いてはいけない場合は null
 */
export function resolveOpenableUrl(href: string | null): string | null {
  if (!href) return null;
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return null;
  }
  return ALLOWED_LINK_PROTOCOLS.includes(url.protocol) ? href : null;
}
