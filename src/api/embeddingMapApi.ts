import { invokeCommand } from "./client";
import type { MapPoint, MailPreview } from "../types/embeddingMap";

/**
 * 埋め込みマップ系 Tauri commands の型付きラッパ。
 */
export const embeddingMapApi = {
  /** 分類済み・未分類を含む全メールの埋め込み座標を取得する */
  points: () => invokeCommand<MapPoint[]>("embedding_map_points"),

  /** 点クリック時の軽量プレビュー（件名・送信者・本文冒頭）を取得する */
  preview: (mailId: string) =>
    invokeCommand<MailPreview>("mail_preview", { mailId }),
};
