import { invokeCommand } from "./client";
import type { Attachment, InlineImage } from "../types/attachment";

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

  /** 添付候補ファイルのサイズを返す（plugin-fs 非依存で Rust に委ねる） */
  statFile: (path: string) => invokeCommand<number>("stat_file", { path }),
};
