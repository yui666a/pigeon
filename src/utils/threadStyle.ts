import type { Thread } from "../types/mail";

/**
 * スレッド行の背景色クラスを返す。既読/未読の判別を一元化し、
 * ThreadItem（通常一覧）と ThreadDragItem（未分類一覧）で共有する。
 *
 * 優先順位: 選択(青) > 既読(グレー) > 未読(デフォルト白)。
 * 未読を含むスレッドはデフォルト背景のまま目立たせ、全メール既読の
 * スレッドにグレー背景を付けて既読/未読を見分けやすくする。
 */
export function threadBackgroundClass(
  thread: Thread,
  selected = false,
): string {
  if (selected) return "bg-blue-50";
  const hasUnread = thread.mails.some((m) => !m.is_read);
  return hasUnread ? "" : "bg-gray-100";
}
