import { invoke } from "@tauri-apps/api/core";
import { toApiError } from "./errors";

/**
 * 型付き invoke ラッパ。すべての Tauri command 呼び出しはここを通す。
 *
 * - 失敗は ApiError に正規化して投げる（呼び出し側は kind で分岐できる）
 * - コマンド名・引数の組み立ては各 api モジュール（mailApi 等）が担い、
 *   ストア・コンポーネントに invoke とコマンド名文字列を持ち込まない
 */
export async function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw toApiError(e);
  }
}
