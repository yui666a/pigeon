use rusqlite::Connection;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::error::AppError;
use crate::secure_store::SecureStore;
use crate::state::{ApprovedAttachments, DbState, SecureStoreState, SyncLocks};
use crate::usecase::{AuditSink, Driver, NoOpProgressSink, ProgressSink, SqliteAuditSink};

/// 全 driver（commands / 将来の MCP・agent）が共有する借用コンテキスト。
/// Tauri が所有する各 managed State への参照を束ね、driver 情報と監査シンクを持つ。
pub struct Ctx<'a> {
    db: &'a DbState,
    /// 遅延初期化ホルダ（GUI 本番）。実体の生成は `secure_store()` の初回呼び出し時。
    /// use case が秘密情報を必要としない限り Stronghold は開かれない（ADR 0006 決定 1）
    secure_store_state: Option<&'a SecureStoreState>,
    /// 初期化済みの実体を直接注入する経路（CLI / MCP、およびテスト）。
    secure_store: Option<&'a SecureStore>,
    approved_attachments: Option<&'a ApprovedAttachments>,
    pending: &'a PendingClassifications,
    batches: &'a ClassifyBatches,
    sync_locks: &'a SyncLocks,
    driver: Driver,
    /// None なら既定の SqliteAuditSink（audit() 参照）。テストで差し替える。
    audit: Option<&'a dyn AuditSink>,
    /// None なら NoOpProgressSink（progress() 参照）。driver ごとに差し替える。
    progress: Option<&'a dyn ProgressSink>,
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
            secure_store_state: Some(secure_store),
            secure_store: None,
            approved_attachments: None,
            pending,
            batches,
            sync_locks,
            driver: Driver::Ui,
            audit: None,
            progress: None,
        }
    }

    /// GUI を伴わない driver（CLI / MCP）用のコンストラクタ。
    /// Tauri State ではなく SecureStore を直接受け取る。
    /// CLI では IMAP 認証に secure_store が要るため、Option にせず必須引数にする。
    pub fn new_headless(
        db: &'a DbState,
        secure_store: &'a SecureStore,
        pending: &'a PendingClassifications,
        batches: &'a ClassifyBatches,
        sync_locks: &'a SyncLocks,
        driver: Driver,
    ) -> Self {
        Self {
            db,
            secure_store_state: None,
            secure_store: Some(secure_store),
            approved_attachments: None,
            pending,
            batches,
            sync_locks,
            driver,
            audit: None,
            progress: None,
        }
    }

    /// 添付の送信許可リストを持たせる（send_mail use case 用のビルダー）。
    pub fn with_approved_attachments(mut self, approved: &'a ApprovedAttachments) -> Self {
        self.approved_attachments = Some(approved);
        self
    }

    /// テスト用: 実 SecureStore（tempfile 上の Stronghold）を注入する。
    /// local-only 分岐の use case テストは参照を取るだけで実体には触れない。
    #[cfg(test)]
    pub fn with_secure_store(mut self, store: &'a SecureStore) -> Self {
        self.secure_store = Some(store);
        self
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
            secure_store_state: None,
            secure_store: None,
            approved_attachments: None,
            pending,
            batches,
            sync_locks,
            driver: Driver::Ui,
            audit: None,
            progress: None,
        }
    }

    /// driver を差し替える。非 UI driver（CLI / MCP）の構築とテストで使う。
    pub fn with_driver(mut self, driver: Driver) -> Self {
        self.driver = driver;
        self
    }

    /// テスト用: 監査シンクを差し替える（InMemoryAuditSink での検証用）。
    #[cfg(test)]
    pub fn with_audit_sink(mut self, sink: &'a dyn AuditSink) -> Self {
        self.audit = Some(sink);
        self
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

    /// SecureStore への参照。**この呼び出しが初回なら、ここで初期化が走る**
    /// （数十秒かかりうる。ADR 0006 決定 1）。初期化に失敗した場合は panic せず
    /// エラーを返し、呼び出し元のコマンド経由でユーザーへ伝える。
    /// テスト用 Ctx で未設定の場合もエラー。
    pub fn secure_store(&self) -> Result<&SecureStore, AppError> {
        if let Some(store) = self.secure_store {
            return Ok(store);
        }
        self.secure_store_state
            .ok_or_else(|| {
                AppError::Validation("secure store not configured in this context".into())
            })?
            .get()
    }

    /// DbState への参照。`with_conn` で足りない、
    /// 接続ロックを跨いで await する service 関数（delete/archive 等）に渡す。
    pub fn db(&self) -> &DbState {
        self.db
    }

    /// 添付の送信許可リスト。send_mail use case のみが要求する。
    pub fn approved_attachments(&self) -> Result<&ApprovedAttachments, AppError> {
        self.approved_attachments.ok_or_else(|| {
            AppError::Validation("approved attachments not configured in this context".into())
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

    /// この Ctx を構築した driver（ゲートの判定材料）。
    pub fn driver(&self) -> Driver {
        self.driver
    }

    /// 監査シンク。既定は SQLite 永続化（audit_log テーブル）。
    pub fn audit(&self) -> &dyn AuditSink {
        const DEFAULT: &SqliteAuditSink = &SqliteAuditSink;
        self.audit.unwrap_or(DEFAULT)
    }

    /// 進捗シンクを差し替える（GUI: Tauri emit / CLI: stderr / MCP: 破棄）。
    pub fn with_progress(mut self, sink: &'a dyn ProgressSink) -> Self {
        self.progress = Some(sink);
        self
    }

    /// 進捗シンク。既定は NoOp（進捗を捨てる）。
    pub fn progress(&self) -> &dyn ProgressSink {
        const DEFAULT: &NoOpProgressSink = &NoOpProgressSink;
        self.progress.unwrap_or(DEFAULT)
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

    #[test]
    fn test_ctx_does_not_initialize_secure_store_until_requested() {
        // GUI 経路の Ctx は遅延ホルダを借用するだけで、use case が
        // secure_store() を呼ぶまで Stronghold を開かない（ADR 0006 決定 1）。
        // 実 Stronghold は使わず InMemory で初期化の有無だけを見る
        let (db, pending, batches, locks) = build_states();
        let state = SecureStoreState::lazy(|| Ok(SecureStore::in_memory()));
        let ctx = Ctx::new(&db, &state, &pending, &batches, &locks);

        // with_conn だけを使う use case では初期化されない
        ctx.with_conn(|_| Ok(())).expect("with_conn");
        assert!(!state.is_initialized(), "Ctx 構築と DB 利用では開かれない");

        ctx.secure_store().expect("secure store");
        assert!(state.is_initialized(), "要求された時点で初めて開かれる");
    }

    #[test]
    fn test_ctx_propagates_secure_store_init_failure() {
        // 遅延初期化の失敗は panic せずコマンドへ伝播する
        let (db, pending, batches, locks) = build_states();
        let state = SecureStoreState::lazy(|| Err(AppError::Stronghold("keychain denied".into())));
        let ctx = Ctx::new(&db, &state, &pending, &batches, &locks);

        let err = match ctx.secure_store() {
            Err(e) => e,
            Ok(_) => panic!("初期化失敗はエラーで返る"),
        };
        assert!(
            err.to_string().contains("keychain denied"),
            "原因が残る: {err}"
        );
    }

    #[test]
    fn test_driver_defaults_to_ui() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        assert_eq!(ctx.driver(), crate::usecase::Driver::Ui);
    }

    #[test]
    fn test_new_headless_sets_driver_and_secure_store() {
        let (db, pending, batches, sync_locks) = build_states();
        let store = SecureStore::in_memory();
        let ctx = Ctx::new_headless(
            &db,
            &store,
            &pending,
            &batches,
            &sync_locks,
            Driver::CliAutomated,
        );
        assert_eq!(ctx.driver(), Driver::CliAutomated);
        assert!(ctx.secure_store().is_ok());
    }

    #[test]
    fn test_ctx_progress_defaults_to_noop_and_can_be_replaced() {
        use crate::usecase::progress::RecordingProgressSink;

        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        // 既定は NoOp。呼んでも panic しない
        ctx.progress().emit("x", &serde_json::json!({}));

        let sink = RecordingProgressSink::new();
        let ctx = ctx.with_progress(&sink);
        ctx.progress()
            .emit("sync-progress", &serde_json::json!({"done": 1}));
        assert_eq!(sink.events.lock().expect("lock").len(), 1);
    }

    #[test]
    fn test_default_audit_sink_persists_to_sqlite() {
        use crate::usecase::{AuditEntry, Risk};
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        // 既定シンクは SqliteAuditSink（audit_log テーブルへ書く）
        ctx.with_conn(|conn| {
            ctx.audit().record(
                conn,
                &AuditEntry::new("x", Risk::Reversible, ctx.driver(), &serde_json::json!({})),
            );
            Ok(())
        })
        .unwrap();
        let rows = ctx
            .with_conn(|conn| crate::db::audit_log::list_recent(conn, 10))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].use_case, "x");
    }
}
