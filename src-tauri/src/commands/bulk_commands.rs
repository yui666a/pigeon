use tauri::State;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::error::AppError;
use crate::state::{DbState, SecureStoreState, SyncLocks};
use crate::usecase::{dispatch, Registry};

/// 一括操作の結果。1件の失敗で残りを止めないため、成功/失敗を積み上げて返す
/// （設計書 2026-07-13-bulk-actions-design.md「部分失敗の扱い」）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BulkResult {
    pub succeeded: Vec<String>,
    /// (mail_id, エラーメッセージ) の組
    pub failed: Vec<(String, String)>,
}

impl BulkResult {
    pub(crate) fn new() -> Self {
        Self {
            succeeded: Vec::new(),
            failed: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, mail_id: String, result: Result<(), AppError>) {
        match result {
            Ok(()) => self.succeeded.push(mail_id),
            Err(e) => self.failed.push((mail_id, e.to_string())),
        }
    }
}

/// 複数メールを一括削除する。dispatch バス経由（1件でもサーバー削除を伴えば
/// Sensitive）。処理本体は BulkDeleteMailsUseCase（usecase/cases/mailbox.rs）。
#[tauri::command]
pub async fn bulk_delete_mails(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "bulk_delete_mails",
        serde_json::json!({ "account_id": account_id, "mail_ids": mail_ids }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected bulk output: {e}")))
}

/// 複数メールを一括アーカイブする。dispatch バス経由（1件でもサーバー反映を
/// 伴えば Sensitive）。処理本体は BulkArchiveMailsUseCase。
#[tauri::command]
pub async fn bulk_archive_mails(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "bulk_archive_mails",
        serde_json::json!({ "account_id": account_id, "mail_ids": mail_ids }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected bulk output: {e}")))
}

/// 複数メールを一括で案件へ割り当てる。dispatch バス経由（Reversible + 監査）。
/// 処理本体は BulkMoveMailsUseCase（usecase/cases/assign.rs）。
#[tauri::command]
pub async fn bulk_move_mails(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    mail_ids: Vec<String>,
    project_id: String,
) -> Result<BulkResult, AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "bulk_move_mails",
        serde_json::json!({ "mail_ids": mail_ids, "project_id": project_id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected bulk output: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{assignments, mails, projects};
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{make_mail, setup_db};

    #[test]
    fn test_bulk_result_collects_success_and_failure() {
        let mut result = BulkResult::new();
        result.push("m1".into(), Ok(()));
        result.push("m2".into(), Err(AppError::MailNotFound("m2".into())));

        assert_eq!(result.succeeded, vec!["m1".to_string()]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, "m2");
    }

    #[test]
    fn test_bulk_move_mails_partial_failure_continues() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        // m2 は存在しないメールIDのまま呼ぶ（DBに未挿入）

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();

        let mut result = BulkResult::new();
        for mail_id in ["m1", "m2"] {
            let outcome = assignments::move_mail_to_project(&conn, mail_id, &proj.id);
            result.push(mail_id.to_string(), outcome);
        }

        assert_eq!(result.succeeded, vec!["m1".to_string()]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, "m2");

        let assigned = assignments::get_mails_by_project(&conn, &proj.id).unwrap();
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].id, "m1");
    }

    #[test]
    fn test_bulk_move_mails_removes_pending_for_succeeded_only() {
        // 割り当てが確定した m1 の Create 提案は除去され、失敗した m2 の提案は残る
        use crate::classifier::service::PendingClassifications;
        use crate::models::classifier::{ClassifyAction, ClassifyResult};

        let conn = setup_db();
        let pending = PendingClassifications::new();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        // m2 はDBに存在しない → move は失敗する

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();

        let suggestion = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Suggested".into(),
                description: "desc".into(),
            },
            confidence: 0.8,
            reason: "test".into(),
        };
        pending.insert("m1".into(), suggestion.clone()).unwrap();
        pending.insert("m2".into(), suggestion).unwrap();

        // bulk_move_mails コマンド本体と同じループ
        let mut result = BulkResult::new();
        for mail_id in ["m1", "m2"] {
            let outcome = crate::commands::classify_commands::move_mail_inner(
                &conn, &pending, mail_id, &proj.id,
            );
            result.push(mail_id.to_string(), outcome);
        }

        assert_eq!(result.succeeded, vec!["m1".to_string()]);
        assert!(!pending.contains("m1").unwrap(), "確定した提案は除去");
        assert!(pending.contains("m2").unwrap(), "失敗した提案は保持");
    }

    #[test]
    fn test_bulk_move_mails_all_succeed() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "World", "2026-07-13T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();

        let mut result = BulkResult::new();
        for mail_id in ["m1", "m2"] {
            let outcome = assignments::move_mail_to_project(&conn, mail_id, &proj.id);
            result.push(mail_id.to_string(), outcome);
        }

        assert_eq!(result.succeeded.len(), 2);
        assert!(result.failed.is_empty());
    }
}
