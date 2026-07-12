/**
 * メール作成のデフォルト本文形式（リッチ / プレーン）を保持する localStorage キー。
 * 通知トグル（`pigeon.notifyNewMail`）と同じ localStorage 方式。
 * 専用の設定画面への統合はバックログ項目14（設計書
 * 2026-07-13-rich-compose-design.md）。
 */
export const COMPOSE_FORMAT_KEY = "pigeon.composeFormat";

export type ComposeFormat = "rich" | "plain";

/** デフォルトはプレーン（"rich" が明示されている場合のみリッチ） */
export function getDefaultComposeFormat(): ComposeFormat {
  return localStorage.getItem(COMPOSE_FORMAT_KEY) === "rich" ? "rich" : "plain";
}

/** デフォルト形式を保存する。"plain" はデフォルトなのでキー削除で表現する */
export function setDefaultComposeFormat(format: ComposeFormat): void {
  if (format === "rich") {
    localStorage.setItem(COMPOSE_FORMAT_KEY, "rich");
  } else {
    localStorage.removeItem(COMPOSE_FORMAT_KEY);
  }
}
