use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::Mutex;

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

#[cfg(test)]
mod tests {
    use super::*;

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
