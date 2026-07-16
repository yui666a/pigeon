/**
 * deep-link イベントで受け取った URL が自アプリの OAuth コールバックかを厳密に検証する。
 *
 * バックエンドは2つの経路でコールバックを `deep-link://new-url` として emit する:
 * - loopback フロー（現行のデスクトップ実装。`start_loopback_callback_listener`）:
 *   `http://127.0.0.1:<動的ポート>/oauth/callback?code=...&state=...`
 * - カスタムスキーム（deep-link 直受け）:
 *   `com.haiso666.pigeon://oauth/callback?...`（host="oauth" / pathname="/callback"）
 *
 * `includes("oauth/callback")` のような部分文字列一致は
 * `https://evil.example/oauth/callback` も通すため、URL としてパースし
 * スキーム・ホスト・パスの完全一致を確認する。loopback は 127.0.0.1 リテラルの
 * http のみ許可し、localhost 名や外部ホスト（DNS 経由の偽装）は拒否する。
 */
export function isOAuthCallbackUrl(raw: string): boolean {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return false;
  }

  // loopback フロー: http://127.0.0.1:<port>/oauth/callback
  if (
    url.protocol === "http:" &&
    url.hostname === "127.0.0.1" &&
    url.pathname === "/oauth/callback"
  ) {
    return true;
  }

  // カスタムスキーム: com.haiso666.pigeon://oauth/callback
  return (
    url.protocol === "com.haiso666.pigeon:" &&
    url.host === "oauth" &&
    url.pathname === "/callback"
  );
}
