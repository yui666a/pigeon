import { invokeCommand } from "./client";
import type { FetchedExternalImage } from "../utils/externalImages";

/** 外部画像取得 Tauri command の型付きラッパ */
export const remoteImageApi = {
  /**
   * 外部画像をRust経由で取得して data URI として返す（表示オプトイン用）。
   * 取得できなかったURLは結果に含まれず、遮断されたままになる
   */
  fetchExternalImages: (urls: string[]) =>
    invokeCommand<FetchedExternalImage[]>("fetch_external_images", { urls }),
};
