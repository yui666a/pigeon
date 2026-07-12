import { useState } from "react";
import {
  NOTIFY_NEW_MAIL_KEY,
  NOTIFY_SUBJECT_PREVIEW_KEY,
  isNotificationEnabled,
  isSubjectPreviewEnabled,
} from "../../utils/notifyNewMail";

/**
 * 新着メールのデスクトップ通知の ON/OFF トグルと、件名プレビューの
 * ON/OFF トグル。localStorage キーを読み書きする
 * （notifyNewMail.ts と同一仕様）。
 * - 通知本体: `pigeon.notifyNewMail`（"false" で無効、キーなしはデフォルト ON）
 * - 件名プレビュー: `pigeon.notifySubjectPreview`（"true" のときのみ有効、
 *   キーなしはデフォルト OFF。プライバシー配慮のため通知本体と逆の既定値）
 */
export function NotificationToggle() {
  const [enabled, setEnabled] = useState(isNotificationEnabled);
  const [previewEnabled, setPreviewEnabled] = useState(isSubjectPreviewEnabled);

  const handleChange = (next: boolean) => {
    if (next) {
      // デフォルト ON のため、有効化はキー削除で表現する
      localStorage.removeItem(NOTIFY_NEW_MAIL_KEY);
    } else {
      localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");
    }
    setEnabled(next);
  };

  const handlePreviewChange = (next: boolean) => {
    if (next) {
      localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");
    } else {
      // デフォルト OFF のため、無効化はキー削除で表現する
      localStorage.removeItem(NOTIFY_SUBJECT_PREVIEW_KEY);
    }
    setPreviewEnabled(next);
  };

  return (
    <div>
      <label className="flex cursor-pointer items-center gap-2 px-4 py-2 text-xs text-gray-600 hover:bg-gray-100">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => handleChange(e.target.checked)}
          aria-label="新着メールのデスクトップ通知"
        />
        新着メールのデスクトップ通知
      </label>
      {enabled && (
        <label className="flex cursor-pointer items-center gap-2 px-4 py-2 pl-8 text-xs text-gray-600 hover:bg-gray-100">
          <input
            type="checkbox"
            checked={previewEnabled}
            onChange={(e) => handlePreviewChange(e.target.checked)}
            aria-label="通知に件名を表示"
          />
          通知に件名を表示
        </label>
      )}
    </div>
  );
}
