use rusqlite::Connection;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::error::AppError;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState, SyncLocks};

/// 全 driver（commands / 将来の MCP・agent）が共有する借用コンテキスト。
/// Tauri が所有する各 managed State への参照を束ねる。
/// この段階では依存アクセサのみを提供し、Risk ゲート等は Phase 4-4 で載せる。
pub struct Ctx<'a> {
    db: &'a DbState,
    secure_store: Option<&'a SecureStore>,
    pending: &'a PendingClassifications,
    batches: &'a ClassifyBatches,
    sync_locks: &'a SyncLocks,
}

impl<'a> Ctx<'a> {
    pub fn new(
        db: &'a DbState,
        secure_store: &'a SecureStoreState,
        pending: &'a PendingClassifications,
        batches: &'a ClassifyBatches,
        sync_locks: &'a SyncLocks,
    ) -> Self {
        Self {
            db,
            secure_store: Some(&secure_store.0),
            pending,
            batches,
            sync_locks,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(
        db: &'a DbState,
        pending: &'a PendingClassifications,
        batches: &'a ClassifyBatches,
        sync_locks: &'a SyncLocks,
    ) -> Self {
        Self {
            db,
            secure_store: None,
            pending,
            batches,
            sync_locks,
        }
    }

    /// DB 接続を借りてクロージャを実行する（`DbState::with_conn` へ委譲）。
    /// クロージャ内で await を挟まないこと（ロック保持のため）。
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.db.with_conn(f)
    }

    /// `with_conn` の可変版。
    pub fn with_conn_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.db.with_conn_mut(f)
    }

    /// SecureStore への参照。テスト用 Ctx で未設定の場合はエラー。
    pub fn secure_store(&self) -> Result<&SecureStore, AppError> {
        self.secure_store.ok_or_else(|| {
            AppError::Validation("secure store not configured in this context".into())
        })
    }

    pub fn pending(&self) -> &PendingClassifications {
        self.pending
    }

    pub fn batches(&self) -> &ClassifyBatches {
        self.batches
    }

    pub fn sync_locks(&self) -> &SyncLocks {
        self.sync_locks
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::test_helpers::setup_db;

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[test]
    fn test_with_conn_runs_closure_against_db() {
        let (db, pending, batches, locks) = build_states();
        // SecureStore はこのテストでは使わないので、
        // secure_store を要求しない with_conn 経路のみ検証する。
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let one: i64 = ctx
            .with_conn(|conn| {
                let v: i64 = conn.query_row("SELECT 1", [], |r| r.get(0))?;
                Ok(v)
            })
            .expect("with_conn should run the closure");
        assert_eq!(one, 1);
    }

    #[test]
    fn test_sync_locks_accessor_returns_shared_state() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        assert!(ctx.sync_locks().try_begin("acc1"));
        // 同じ基盤 State を指しているので、二重開始は拒否される
        assert!(!ctx.sync_locks().try_begin("acc1"));
    }
}
