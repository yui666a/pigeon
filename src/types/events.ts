/**
 * バックエンドが emit する Tauri イベントのペイロード型。
 * イベント名と対で使う（listen<SyncProgress>("sync-progress", ...) 等）。
 */

/** "sync-progress" / "backfill-progress" イベントのペイロード */
export interface SyncProgress {
  account_id: string;
  done: number;
  total: number;
}

/** "new-mail-detected" イベント（IMAP IDLE の新着検知）のペイロード */
export interface NewMailEvent {
  account_id: string;
}
