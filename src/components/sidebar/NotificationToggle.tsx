import { useState } from "react";
import {
  NOTIFY_NEW_MAIL_KEY,
  isNotificationEnabled,
} from "../../utils/notifyNewMail";

/**
 * 新着メールのデスクトップ通知の ON/OFF トグル（1行）。
 * localStorage キー `pigeon.notifyNewMail` を読み書きする
 * （"false" で無効、キーなしはデフォルト ON。notifyNewMail.ts と同一仕様）。
 */
export function NotificationToggle() {
  const [enabled, setEnabled] = useState(isNotificationEnabled);

  const handleChange = (next: boolean) => {
    if (next) {
      // デフォルト ON のため、有効化はキー削除で表現する
      localStorage.removeItem(NOTIFY_NEW_MAIL_KEY);
    } else {
      localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");
    }
    setEnabled(next);
  };

  return (
    <label className="flex cursor-pointer items-center gap-2 px-4 py-2 text-xs text-gray-600 hover:bg-gray-100">
      <input
        type="checkbox"
        checked={enabled}
        onChange={(e) => handleChange(e.target.checked)}
        aria-label="新着メールのデスクトップ通知"
      />
      新着メールのデスクトップ通知
    </label>
  );
}
