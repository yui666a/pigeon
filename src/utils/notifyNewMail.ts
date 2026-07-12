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

/** "false" が明示されていない限り有効（デフォルト ON） */
export function isNotificationEnabled(): boolean {
  return localStorage.getItem(NOTIFY_NEW_MAIL_KEY) !== "false";
}

/**
 * 新着メールのデスクトップ通知を表示する。
 * 通知は補助機能のため、権限拒否・プラグインエラーは静かにスキップし、
 * エラートーストは出さない。
 */
export async function notifyNewMail(count: number): Promise<void> {
  if (!isNotificationEnabled()) return;
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (!granted) return;
    sendNotification({
      title: "Pigeon",
      body: `${count}件の新着メールを受信しました`,
    });
  } catch (e) {
    console.error("notifyNewMail failed:", e);
  }
}
