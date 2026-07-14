/**
 * deep-link で受け取った URL が自アプリの OAuth コールバックかを厳密に検証する。
 *
 * `includes("oauth/callback")` のような部分文字列一致は
 * `https://evil.example/oauth/callback` も通してしまうため、URL としてパースし
 * スキームとパスの完全一致を確認する。カスタムスキーム URL では `//` 直後が
 * authority（host）として解釈されるため、`com.haiso666.pigeon://oauth/callback`
 * は host="oauth" / pathname="/callback" になる。
 */
export function isOAuthCallbackUrl(raw: string): boolean {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return false;
  }
  return (
    url.protocol === "com.haiso666.pigeon:" &&
    url.host === "oauth" &&
    url.pathname === "/callback"
  );
}
