/**
 * スレッド一覧を1回のリクエストで取得する件数（スレッド単位）。
 *
 * バックエンドの既定値（src-tauri/src/commands/mail_commands.rs の
 * DEFAULT_THREAD_PAGE_SIZE）と揃える。一覧取得は必ず上限を持ち、続きは
 * offset を進めて追加取得する（ADR 0006 決定5）。
 */
export const THREAD_PAGE_SIZE = 200;
