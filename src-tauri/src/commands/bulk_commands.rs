use tauri::State;

use crate::db::{accounts, assignments, mails, settings};
use crate::error::AppError;
use crate::mail_sync::imap_client;
use crate::models::account::{Account, AccountProvider};
use crate::models::mail::Mail;
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
    fn new() -> Self {
        Self {
            succeeded: Vec::new(),
            failed: Vec::new(),
        }
    }

    fn push(&mut self, mail_id: String, result: Result<(), AppError>) {
        match result {
            Ok(()) => self.succeeded.push(mail_id),
            Err(e) => self.failed.push((mail_id, e.to_string())),
        }
    }
}

// 以下の plan_delete / plan_archive は mail_commands.rs の同名関数と
// 判定ロジックが完全に一致している必要がある（意図的な重複）。
// Why not shared: 実装時点で mail_commands.rs は並行編集中の他ブランチ
// （Sent フォルダ同期対応。Sent の LocalOnly 判定を変更中）が触っており、
// 共通化すると衝突するため個別に複製した。sent-sync 側で判定ロジックが
// 変わった場合はこちらも必ず追随させること。共通化はブランチ統合時に
// リードがスタック順を決めて対応する（設計書
// 2026-07-13-bulk-actions-design.md「v1の制限」参照）。

/// 削除のサーバー反映方式（mail_commands::plan_delete と同じ判定）
fn plan_delete(folder: &str) -> bool {
    folder != "Sent"
}

/// アーカイブのサーバー反映方式（mail_commands::plan_archive と同じ判定）
enum ArchivePlan {
    DeleteOnly,
    CopyThenDelete(String),
    LocalOnly,
}

fn plan_archive(provider: &AccountProvider, folder: &str, archive_folder: &str) -> ArchivePlan {
    if folder == "Sent" {
        return ArchivePlan::LocalOnly;
    }
    match provider {
        AccountProvider::Google => ArchivePlan::DeleteOnly,
        AccountProvider::Other => ArchivePlan::CopyThenDelete(archive_folder.to_string()),
    }
}

async fn resolve_credentials(
    account: &Account,
    secure_store: &crate::secure_store::SecureStore,
) -> Result<(crate::models::account::AuthType, String, String), AppError> {
    crate::commands::mail_commands::resolve_imap_credentials(account, secure_store).await
}

/// 複数メールを一括削除する。1件ずつサーバー処理→ローカルDB削除の順で実行し、
/// 失敗したメールはスキップして残りを継続する。
#[tauri::command]
pub async fn bulk_delete_mails(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError> {
    let account = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        accounts::get_account(&conn, &account_id)?
    };
    let (auth_type, username, credential) = resolve_credentials(&account, &secure_store.0).await?;

    let mut result = BulkResult::new();
    for mail_id in mail_ids {
        let outcome = delete_one(&state, &account, &auth_type, &username, &credential, &mail_id).await;
        result.push(mail_id, outcome);
    }
    Ok(result)
}

async fn delete_one(
    state: &State<'_, DbState>,
    account: &Account,
    auth_type: &crate::models::account::AuthType,
    username: &str,
    credential: &str,
    mail_id: &str,
) -> Result<(), AppError> {
    let mail = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        mails::get_mail_by_id(&conn, mail_id)?
    };

    if plan_delete(&mail.folder) {
        imap_client::delete_message_remote(
            &account.imap_host,
            account.imap_port,
            auth_type,
            username,
            credential,
            &mail.folder,
            mail.uid,
        )
        .await?;
    }

    {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        mails::delete_mail(&conn, mail_id)?;
    }
    // DB削除成功後、添付キャッシュをベストエフォートで掃除する
    // （失敗しても削除自体は成功扱い。孤児化したディスクリークの防止）
    crate::commands::attachment_commands::remove_attachment_cache_for_mail(mail_id);
    Ok(())
}

/// 複数メールを一括アーカイブする。処理方式は単体の archive_mail と同じ
/// （Google: DeleteOnly / Other: CopyThenDelete / Sent: LocalOnly）。
#[tauri::command]
pub async fn bulk_archive_mails(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError> {
    let (account, archive_folder) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let account = accounts::get_account(&conn, &account_id)?;
        let archive_folder = settings::get_or_default(&conn, "archive_folder", "Archive")?;
        (account, archive_folder)
    };
    let (auth_type, username, credential) = resolve_credentials(&account, &secure_store.0).await?;

    let mut result = BulkResult::new();
    for mail_id in mail_ids {
        let outcome = archive_one(
            &state,
            &account,
            &archive_folder,
            &auth_type,
            &username,
            &credential,
            &mail_id,
        )
        .await;
        result.push(mail_id, outcome);
    }
    Ok(result)
}

async fn archive_one(
    state: &State<'_, DbState>,
    account: &Account,
    archive_folder: &str,
    auth_type: &crate::models::account::AuthType,
    username: &str,
    credential: &str,
    mail_id: &str,
) -> Result<(), AppError> {
    let mail: Mail = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        mails::get_mail_by_id(&conn, mail_id)?
    };

    let plan = plan_archive(&account.provider, &mail.folder, archive_folder);
    if !matches!(plan, ArchivePlan::LocalOnly) {
        let copy_dest = match &plan {
            ArchivePlan::CopyThenDelete(dest) => Some(dest.as_str()),
            _ => None,
        };
        imap_client::archive_message_remote(
            &account.imap_host,
            account.imap_port,
            auth_type,
            username,
            credential,
            &mail.folder,
            mail.uid,
            copy_dest,
        )
        .await?;
    }

    let conn = state.0.lock().map_err(AppError::lock_err)?;
    mails::update_folder(&conn, mail_id, "Archive")
}

/// 複数メールを一括で案件へ割り当てる。IMAP 通信を伴わないため同期関数のまま。
#[tauri::command]
pub fn bulk_move_mails(
    state: State<DbState>,
    mail_ids: Vec<String>,
    project_id: String,
) -> Result<BulkResult, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    let mut result = BulkResult::new();
    for mail_id in mail_ids {
        let outcome = assignments::move_mail_to_project(&conn, &mail_id, &project_id);
        result.push(mail_id, outcome);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::projects;
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{make_mail, setup_db};

    #[test]
    fn test_plan_delete_inbox_requires_server() {
        assert!(plan_delete("INBOX"));
        assert!(plan_delete("Archive"));
    }

    #[test]
    fn test_plan_delete_sent_is_local_only() {
        assert!(!plan_delete("Sent"));
    }

    #[test]
    fn test_plan_archive_google_deletes_without_copy() {
        assert!(matches!(
            plan_archive(&AccountProvider::Google, "INBOX", "Archive"),
            ArchivePlan::DeleteOnly
        ));
    }

    #[test]
    fn test_plan_archive_other_copies_to_archive_folder() {
        assert!(matches!(
            plan_archive(&AccountProvider::Other, "INBOX", "MyArchive"),
            ArchivePlan::CopyThenDelete(dest) if dest == "MyArchive"
        ));
    }

    #[test]
    fn test_plan_archive_sent_is_local_only() {
        assert!(matches!(
            plan_archive(&AccountProvider::Google, "Sent", "Archive"),
            ArchivePlan::LocalOnly
        ));
    }

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
