//! アカウント同期の use case。多重起動ガード（SyncLocks）と進捗送出を
//! ここに集約し、driver（GUI / CLI / MCP）に依存しない形にする。
//! 進捗は ProgressSink 経由で送るため、イベント名 "sync-progress" と
//! payload のキー（account_id / done / total）は frontend の
//! `SyncProgress` 型（src/types/events.ts）と一致させること。

use serde::Deserialize;

use crate::commands::mail_commands::resolve_imap_credentials;
use crate::context::Ctx;
use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::sync_service;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SyncAccountInput {
    pub account_id: String,
}

pub struct SyncAccountUseCase;

#[async_trait::async_trait]
impl UseCase for SyncAccountUseCase {
    type Input = SyncAccountInput;
    type Output = u32;

    fn name(&self) -> &'static str {
        "sync_account"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        // 同一アカウントの同期が進行中なら開始しない（画面遷移等での多重起動対策）。
        // エラーではなく 0 件を返す: 呼び出し側にとって「新規取り込みなし」と等価
        if !ctx.sync_locks().try_begin(&input.account_id) {
            return Ok(0);
        }
        let result = run_locked(&input.account_id, ctx).await;
        ctx.sync_locks().finish(&input.account_id);
        result
    }
}

/// ロック取得後の同期本体。ドメインロジックは mail_sync::sync_service に委譲し、
/// ここでは資格情報解決の注入と進捗送出のみを行う。
async fn run_locked(account_id: &str, ctx: &Ctx<'_>) -> Result<u32, AppError> {
    let account = ctx.with_conn(|conn| accounts::get_account(conn, account_id))?;
    let secure_store = ctx.secure_store()?;
    sync_service::sync_account(
        ctx.db(),
        &account,
        || resolve_imap_credentials(&account, secure_store),
        |done, total| {
            // 進捗はベストエフォート（送出失敗で同期は止めない）
            ctx.progress().emit(
                "sync-progress",
                &sync_progress_payload(account_id, done, total),
            );
        },
    )
    .await
}

/// sync-progress イベントの payload。
///
/// キー名は frontend の `SyncProgress`（src/types/events.ts）がそのまま読むため、
/// 変更すると進捗表示が壊れる。純関数に切り出して wire contract をテストで固定している
/// （コールバックは同期ループの奥でしか発火せず、そのままでは検証できないため）。
fn sync_progress_payload(account_id: &str, done: usize, total: usize) -> serde_json::Value {
    serde_json::json!({
        "account_id": account_id,
        "done": done,
        "total": total,
    })
}

pub fn register_sync_cases(registry: &mut Registry) {
    registry.register(SyncAccountUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[tokio::test]
    async fn test_sync_returns_zero_when_lock_is_held() {
        let (db, pending, batches, sync_locks) = build_states();
        // 別の同期が進行中の状況を作る
        assert!(sync_locks.try_begin("acct-1"));

        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let out = SyncAccountUseCase
            .run(
                SyncAccountInput {
                    account_id: "acct-1".into(),
                },
                &ctx,
            )
            .await
            .expect("run");
        assert_eq!(out, 0, "進行中なら 0 件を返す（エラーにしない）");
    }

    #[tokio::test]
    async fn test_sync_risk_is_reversible() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = SyncAccountInput {
            account_id: "acct-1".into(),
        };
        assert_eq!(
            SyncAccountUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Reversible
        );
    }

    #[tokio::test]
    async fn test_lock_is_released_after_run() {
        // ロック取得に成功した実行は、失敗しても finish でロックを解放する
        // （アカウント未登録で run_locked は Err になる経路）
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let _ = SyncAccountUseCase
            .run(
                SyncAccountInput {
                    account_id: "missing".into(),
                },
                &ctx,
            )
            .await;
        assert!(
            sync_locks.try_begin("missing"),
            "run 後にロックが解放されていること"
        );
    }

    #[test]
    fn test_progress_payload_matches_frontend_contract() {
        // キー名は src/types/events.ts の SyncProgress と一致させる。
        // 変更すると進捗表示が無言で壊れる（コンパイルも実行も通ってしまう）
        let payload = sync_progress_payload("acct-1", 3, 10);
        assert_eq!(
            payload,
            serde_json::json!({
                "account_id": "acct-1",
                "done": 3,
                "total": 10,
            })
        );
    }

    #[test]
    fn test_registered_in_registry() {
        let mut registry = Registry::new();
        register_sync_cases(&mut registry);
        assert!(registry.names().contains(&"sync_account"));
    }
}
