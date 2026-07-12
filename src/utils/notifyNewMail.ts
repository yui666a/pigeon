import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/**
 * デスクトップ通知の ON/OFF を保持する localStorage キー。
 * 設定 UI はサイドバーの NotificationToggle
 * （2026-07-12-desktop-notification-design.md）。
 */
export const NOTIFY_NEW_MAIL_KEY = "pigeon.notifyNewMail";

/**
 * 通知に件名プレビューを表示するかの localStorage キー。
 * プライバシー配慮のためデフォルト OFF（"true" のときのみ有効）。
 * 保存先は既存の通知トグルと統一（localStorage。設定テーブル移行は
 * バックログ #16 で別途対応）
 */
export const NOTIFY_SUBJECT_PREVIEW_KEY = "pigeon.notifySubjectPreview";

/** 件名プレビューに表示する先頭件数（残りは「他N件」でまとめる） */
const SUBJECT_PREVIEW_LIMIT = 3;

/** "false" が明示されていない限り有効（デフォルト ON） */
export function isNotificationEnabled(): boolean {
  return localStorage.getItem(NOTIFY_NEW_MAIL_KEY) !== "false";
}

/** "true" が明示されている場合のみ有効（デフォルト OFF。プライバシー配慮） */
export function isSubjectPreviewEnabled(): boolean {
  return localStorage.getItem(NOTIFY_SUBJECT_PREVIEW_KEY) === "true";
}

/**
 * 通知本文を組み立てる純関数。
 * - プレビュー無効、または対象の件名が1件も無い場合は件数のみ
 * - 先頭 SUBJECT_PREVIEW_LIMIT 件の件名を改行区切りで表示し、
 *   残りがあれば「他N件」を末尾に追加する
 */
export function buildNotificationBody(
  count: number,
  subjects: string[],
  previewEnabled: boolean,
): string {
  const countOnly = `${count}件の新着メールを受信しました`;
  if (!previewEnabled || subjects.length === 0) return countOnly;

  const shown = subjects.slice(0, SUBJECT_PREVIEW_LIMIT);
  const remaining = count - shown.length;
  const lines = shown.join("\n");
  return remaining > 0 ? `${lines}\n他${remaining}件` : lines;
}

/**
 * 新着メールのデスクトップ通知を表示する。
 * 通知は補助機能のため、権限拒否・プラグインエラーは静かにスキップし、
 * エラートーストは出さない。
 *
 * @param subjects プレビュー対象の件名（新着として取り込まれたメールの件名）。
 *   省略時・プレビュー設定OFF時は件数のみの通知になる
 */
export async function notifyNewMail(
  count: number,
  subjects: string[] = [],
): Promise<void> {
  if (!isNotificationEnabled()) return;
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (!granted) return;
    sendNotification({
      title: "Pigeon",
      body: buildNotificationBody(count, subjects, isSubjectPreviewEnabled()),
    });
  } catch (e) {
    console.error("notifyNewMail failed:", e);
  }
}
