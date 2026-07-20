//! 未分類メールのバッチ分類の use case。分類器の構築と進捗送出を
//! ここに集約し、driver（GUI / CLI / MCP）に依存しない形にする。
//! 進捗は ProgressSink 経由で送るため、イベント名 "classify-progress" と
//! payload のキー（account_id / current / total / assigned_mail_id）は
//! frontend の `ClassifyProgressEvent` 型（src/types/classifier.ts）と
//! 一致させること。

use serde::Deserialize;

use crate::classifier::factory::build_classifier;
use crate::classifier::service;
use crate::context::Ctx;
use crate::error::AppError;
use crate::models::classifier::ClassifyBatchOutcome;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ClassifyBatchInput {
    pub account_id: String,
}

pub struct ClassifyBatchUseCase;

/// classify-progress の payload を組み立てる。
///
/// キー名は frontend の `ClassifyProgressEvent`（src/types/classifier.ts）が
/// そのまま読むため、変更すると進捗表示と未分類一覧の即時反映が壊れる。
/// 純関数に切り出して wire contract をテストで固定している。
fn classify_progress_payload(
    account_id: &str,
    current: usize,
    total: usize,
    assigned_mail_id: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "account_id": account_id,
        "current": current,
        "total": total,
        "assigned_mail_id": assigned_mail_id,
    })
}

#[async_trait::async_trait]
impl UseCase for ClassifyBatchUseCase {
    type Input = ClassifyBatchInput;
    type Output = ClassifyBatchOutcome;

    fn name(&self) -> &'static str {
        "classify_batch"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        // 案件割り当ての変更は取り消せる
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let secure_store = ctx.secure_store()?;
        let classifier = ctx.with_conn(|conn| build_classifier(conn, secure_store))?;
        let account_id = input.account_id;
        service::classify_batch(
            &ctx.db().0,
            classifier.as_ref(),
            ctx.pending(),
            ctx.batches(),
            &account_id,
            |current, total, assigned_mail_id| {
                // 進捗はベストエフォート（送出失敗で分類は止めない）
                ctx.progress().emit(
                    "classify-progress",
                    &classify_progress_payload(&account_id, current, total, assigned_mail_id),
                );
            },
        )
        .await
    }
}

pub fn register_classify_cases(registry: &mut Registry) {
    registry.register(ClassifyBatchUseCase);
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
    async fn test_classify_batch_risk_is_reversible() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = ClassifyBatchInput {
            account_id: "acct-1".into(),
        };
        assert_eq!(
            ClassifyBatchUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Reversible
        );
    }

    #[test]
    fn test_progress_payload_matches_frontend_contract() {
        // キー名は src/types/classifier.ts の ClassifyProgressEvent と一致させる。
        // 変更すると進捗表示と未分類一覧の即時反映が無言で壊れる
        let payload = classify_progress_payload("acct-1", 2, 5, Some("mail-42"));
        assert_eq!(
            payload,
            serde_json::json!({
                "account_id": "acct-1",
                "current": 2,
                "total": 5,
                "assigned_mail_id": "mail-42",
            })
        );
    }

    #[test]
    fn test_progress_payload_assigned_mail_id_is_null_when_absent() {
        // フロントは null を「確定割り当てなし」と読む（型は string | null）
        let payload = classify_progress_payload("acct-1", 1, 3, None);
        assert!(payload["assigned_mail_id"].is_null());
    }

    #[test]
    fn test_registered_in_registry() {
        let mut registry = Registry::new();
        register_classify_cases(&mut registry);
        assert!(registry.names().contains(&"classify_batch"));
    }
}
