import { invokeCommand } from "./client";
import type { Attachment, InlineImage } from "../types/attachment";

/** pick_attachment_files が返す選択結果（送信許可リスト登録済み） */
export interface PickedAttachment {
  path: string;
  name: string;
  size: number;
}

/** 添付ファイル系 Tauri commands の型付きラッパ */
export const attachmentApi = {
  listAttachments: (mailId: string) =>
    invokeCommand<Attachment[]>("list_attachments", { mailId }),

  /** 保存先はバックエンドがネイティブダイアログで選ぶ。キャンセル時は false */
  saveAttachment: (attachmentId: string) =>
    invokeCommand<boolean>("save_attachment", { attachmentId }),

  /** 本文中の cid 参照を解決するためのインライン画像を返す */
  fetchInlineImages: (mailId: string) =>
    invokeCommand<InlineImage[]>("get_inline_images", { mailId }),

  /**
   * 添付ファイルをネイティブダイアログで選択する。選択されたパスのみが
   * バックエンドの送信許可リストに登録される（任意パス読み取りの防止）。
   * キャンセル時は空配列
   */
  pickAttachmentFiles: () =>
    invokeCommand<PickedAttachment[]>("pick_attachment_files"),
};
