/**
 * スレッド一覧を1回のリクエストで取得する件数（スレッド単位）。
 *
 * バックエンドの既定値（src-tauri/src/usecase/cases/mailbox.rs の
 * DEFAULT_THREAD_PAGE_SIZE）と揃える。一覧取得は必ず上限を持ち、続きは
 * offset を進めて追加取得する（ADR 0006 決定5）。
 *
 * バックエンド側は MAX_THREAD_PAGE_SIZE（500）でクランプするため、
 * ここを 500 より大きくしても実際に返るのは 500 件までになる。
 */
export const THREAD_PAGE_SIZE = 200;
