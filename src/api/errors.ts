/**
 * バックエンド（Tauri command）呼び出しの失敗を型で表現する。
 *
 * Rust 側の command は `Result<T, String>` を返すため、invoke の失敗は
 * 生の文字列で届く。ここで ApiError に正規化し、再認証要求のような
 * 「種別で分岐したい失敗」をマジックストリング照合ではなく kind で
 * 判定できるようにする。
 */

/** 失敗の種別。分岐が必要になったものだけを追加する */
export type ApiErrorKind = "reauth" | "unknown";

export class ApiError extends Error {
  readonly kind: ApiErrorKind;

  constructor(kind: ApiErrorKind, message: string) {
    super(message);
    this.name = "ApiError";
    this.kind = kind;
  }
}

/** バックエンドが OAuth 再認証を要求したときのエラーメッセージの目印
 * （src-tauri の mail_sync が返す "Reauth required: <account_id>" に対応） */
const REAUTH_MARKER = "Reauth required";

/** invoke の失敗値（文字列 / Error / その他）を ApiError に正規化する */
export function toApiError(e: unknown): ApiError {
  if (e instanceof ApiError) return e;
  const message = e instanceof Error ? e.message : String(e);
  const kind: ApiErrorKind = message.includes(REAUTH_MARKER)
    ? "reauth"
    : "unknown";
  return new ApiError(kind, message);
}

/** OAuth 再認証が必要な失敗かどうか */
export function isReauthError(e: unknown): boolean {
  return toApiError(e).kind === "reauth";
}

/** トースト等の表示用メッセージを取り出す（従来の String(e) と同じ文字列になる） */
export function errorMessage(e: unknown): string {
  return toApiError(e).message;
}
