use tauri::State;

use crate::commands::mail_commands;
use crate::db::{accounts, settings};
use crate::error::AppError;
use crate::state::{DbState, SecureStoreState};

/// 一括操作の結果。1件の失敗で残りを止めないため、成功/失敗を積み上げて返す
/// （設計書 2026-07-13-bulk-actions-design.md「部分失敗の扱い」）。
#[derive(Debug, Clone, serde::Serialize)]
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

/// 複数メールを一括削除する。処理本体は単体の delete_mail と共通
/// （mail_commands::delete_mail_inner）。1件ずつサーバー処理→ローカルDB削除の
/// 順で実行し、失敗したメールはスキップして残りを継続する。
/// 資格情報はコマンド開始時に一度だけ解決して全メールで使い回す
/// （解決失敗は部分失敗ではなく全体エラー）。
#[tauri::command]
pub async fn bulk_delete_mails(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError> {
    let account = state.with_conn(|conn| accounts::get_account(conn, &account_id))?;
    let creds = tokio::sync::OnceCell::new_with(Some(
        mail_commands::resolve_imap_credentials(&account, &secure_store.0).await?,
    ));

    let mut result = BulkResult::new();
    for mail_id in mail_ids {
        let outcome =
            mail_commands::delete_mail_inner(&state, &secure_store.0, &account, &creds, &mail_id)
                .await;
        result.push(mail_id, outcome);
    }
    Ok(result)
}

/// 複数メールを一括アーカイブする。処理本体は単体の archive_mail と共通
/// （mail_commands::archive_mail_inner。Google: DeleteOnly / Other:
/// CopyThenDelete / Sent: LocalOnly）。
#[tauri::command]
pub async fn bulk_archive_mails(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError> {
    let (account, archive_folder) = state.with_conn(|conn| {
        let account = accounts::get_account(conn, &account_id)?;
        let archive_folder = settings::get_or_default(conn, "archive_folder", "Archive")?;
        Ok((account, archive_folder))
    })?;
    let creds = tokio::sync::OnceCell::new_with(Some(
        mail_commands::resolve_imap_credentials(&account, &secure_store.0).await?,
    ));

    let mut result = BulkResult::new();
    for mail_id in mail_ids {
        let outcome = mail_commands::archive_mail_inner(
            &state,
            &secure_store.0,
            &account,
            &archive_folder,
            &creds,
            &mail_id,
        )
        .await;
        result.push(mail_id, outcome);
    }
    Ok(result)
}

/// 複数メールを一括で案件へ割り当てる。IMAP 通信を伴わないため同期関数のまま。
/// 単体の `move_mail` と同じ本体（`move_mail_inner`）を1件ずつ再利用するため、
/// 保留中の分類提案（`PendingClassifications`）の掃除も同じ挙動になる。
#[tauri::command]
pub fn bulk_move_mails(
    state: State<DbState>,
    pending: State<crate::classifier::service::PendingClassifications>,
    mail_ids: Vec<String>,
    project_id: String,
) -> Result<BulkResult, AppError> {
    state.with_conn(|conn| {
        let mut result = BulkResult::new();
        for mail_id in mail_ids {
            let outcome = crate::commands::classify_commands::move_mail_inner(
                conn,
                &pending,
                &mail_id,
                &project_id,
            );
            result.push(mail_id, outcome);
        }
        Ok(result)
    })
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
