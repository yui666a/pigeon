use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use tauri::async_runtime::JoinHandle;

use crate::secure_store::SecureStore;

pub struct DbState(pub Mutex<Connection>);
pub struct SecureStoreState(pub SecureStore);

/// アカウント単位の同期実行ロック。
/// スレッド一覧は表示のたびに sync_account を呼ぶため、画面遷移や
/// React 開発モードの二重 effect で同一アカウントの同期が並行し得る。
/// 並行すると全員が同期前の max_uid を見て同じメールを多重取り込みするので、
/// ここで直列化する（2本目以降は開始せず即リターンさせる）。
#[derive(Default)]
pub struct SyncLocks(Mutex<HashSet<String>>);

impl SyncLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 同期を開始できれば true。同じアカウントの同期が進行中なら false。
    pub fn try_begin(&self, account_id: &str) -> bool {
        match self.0.lock() {
            Ok(mut in_flight) => in_flight.insert(account_id.to_string()),
            // ロックが毒化していたら安全側（開始しない）
            Err(_) => false,
        }
    }

    /// 同期の終了を記録する（成功・失敗を問わず必ず呼ぶ）。
    pub fn finish(&self, account_id: &str) {
        if let Ok(mut in_flight) = self.0.lock() {
            in_flight.remove(account_id);
        }
    }
}

/// アカウント毎の IMAP IDLE 監視タスク（mail_sync::idle）の管理。
/// 開始（insert）・停止（stop）を account_id 単位で行う。
/// 起動時・アカウント追加・OAuth 完了時に開始し、アカウント削除時に停止する
#[derive(Default)]
pub struct IdleWatchers(Mutex<HashMap<String, JoinHandle<()>>>);

impl IdleWatchers {
    pub fn new() -> Self {
        Self::default()
    }

    /// 監視タスクを登録する。同一アカウントの既存タスクは中断して置き換える
    /// （OAuth 再認証後の監視再開もこの置き換えで実現する）
    pub fn insert(&self, account_id: &str, handle: JoinHandle<()>) {
        match self.0.lock() {
            Ok(mut map) => {
                if let Some(old) = map.insert(account_id.to_string(), handle) {
                    old.abort();
                }
            }
            // ロックが毒化していたら登録できない。渡されたタスクは停止しておく
            Err(_) => handle.abort(),
        }
    }

    /// 監視タスクを中断して登録解除する（アカウント削除時）
    pub fn stop(&self, account_id: &str) {
        if let Ok(mut map) = self.0.lock() {
            if let Some(handle) = map.remove(account_id) {
                handle.abort();
            }
        }
    }

    /// 監視タスクが登録されているか
    pub fn is_watching(&self, account_id: &str) -> bool {
        self.0
            .lock()
            .map(|map| map.contains_key(account_id))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_task() -> JoinHandle<()> {
        tauri::async_runtime::spawn(async {
            // 監視タスクの代役: abort されるまで待つだけ
            std::future::pending::<()>().await;
        })
    }

    #[test]
    fn test_idle_watchers_insert_and_stop() {
        let watchers = IdleWatchers::new();
        assert!(!watchers.is_watching("acc1"));

        watchers.insert("acc1", dummy_task());
        assert!(watchers.is_watching("acc1"));
        assert!(!watchers.is_watching("acc2"), "別アカウントには影響しない");

        watchers.stop("acc1");
        assert!(!watchers.is_watching("acc1"));
    }

    #[test]
    fn test_idle_watchers_insert_replaces_existing_task() {
        let watchers = IdleWatchers::new();
        watchers.insert("acc1", dummy_task());
        // 置き換えても二重登録にならない（旧タスクは abort される）
        watchers.insert("acc1", dummy_task());
        assert!(watchers.is_watching("acc1"));
        watchers.stop("acc1");
        assert!(!watchers.is_watching("acc1"));
    }

    #[test]
    fn test_idle_watchers_stop_unknown_account_is_noop() {
        let watchers = IdleWatchers::new();
        watchers.stop("missing"); // panic しない
        assert!(!watchers.is_watching("missing"));
    }

    #[test]
    fn test_second_begin_is_rejected_while_in_flight() {
        let locks = SyncLocks::new();
        assert!(locks.try_begin("acc1"));
        assert!(!locks.try_begin("acc1"), "同一アカウントの多重開始は拒否");
    }

    #[test]
    fn test_different_accounts_run_concurrently() {
        let locks = SyncLocks::new();
        assert!(locks.try_begin("acc1"));
        assert!(locks.try_begin("acc2"), "別アカウントは並行してよい");
    }

    #[test]
    fn test_finish_allows_next_sync() {
        let locks = SyncLocks::new();
        assert!(locks.try_begin("acc1"));
        locks.finish("acc1");
        assert!(locks.try_begin("acc1"), "終了後は再び開始できる");
    }
}
