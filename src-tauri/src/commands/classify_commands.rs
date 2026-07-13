use tauri::State;

use crate::classifier::factory::build_classifier;
use crate::classifier::service::{self, PendingClassifications};
use crate::db::{assignments, mails, projects};
use crate::error::AppError;
use crate::models::classifier::ClassifyResponse;
use crate::models::mail::Mail;
use crate::models::project::{CreateProjectRequest, Project};
use crate::state::{DbState, SecureStoreState};

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Classify a single mail by ID and persist the result.
///
/// ユースケースの本体（サマリ構築 → LLM実行 → 確信度ゲート → 振り分け）は
/// `classifier::service::classify_one` にあり、ここでは分類器の構築と
/// サービス呼び出しのみを行う。
#[tauri::command]
pub async fn classify_mail(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    secure_store: State<'_, SecureStoreState>,
    mail_id: String,
) -> Result<ClassifyResponse, AppError> {
    let classifier = db.with_conn(|conn| build_classifier(conn, &secure_store.0))?;
    service::classify_one(&db.0, classifier.as_ref(), &pending, &mail_id).await
}

/// Approve an AI classification (user confirms the assigned project).
#[tauri::command]
pub fn approve_classification(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_id: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| approve_classification_inner(conn, &pending, &mail_id, &project_id))
}

fn approve_classification_inner(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    assignments::approve_classification(conn, mail_id, project_id)?;
    // 割り当てが確定したので、残っている提案があれば除去する
    pending.remove(mail_id)
}

/// Approve a "create new project" suggestion: creates the project and assigns the mail.
#[tauri::command]
pub fn approve_new_project(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_name: String,
    description: Option<String>,
) -> Result<Project, AppError> {
    let project = db.with_conn_mut(|conn| {
        // Load mail to get account_id
        let mail = mails::get_mail_by_id(conn, &mail_id)?;

        let tx = conn.transaction()?;

        let req = CreateProjectRequest {
            account_id: mail.account_id.clone(),
            name: project_name,
            description,
            color: None,
        };
        let project = projects::insert_project(&tx, &req)?;

        assignments::assign_mail(&tx, &mail_id, &project.id, "user", Some(1.0))?;

        tx.commit()?;
        Ok(project)
    })?;

    // Remove from pending map
    pending.remove(&mail_id)?;
    Ok(project)
}

/// Reject an AI classification (remove from pending or delete assignment).
#[tauri::command]
pub fn reject_classification(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
) -> Result<(), AppError> {
    // Remove from pending map if present
    pending.remove(&mail_id)?;

    // Also remove from DB assignments if present
    let result = db.with_conn(|conn| assignments::reject_classification(conn, &mail_id));
    match result {
        Ok(()) => Ok(()),
        // MailNotFound means there was no assignment -- that's fine after removing from pending
        Err(AppError::MailNotFound(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Get all mails that have not yet been assigned to a project.
#[tauri::command]
pub fn get_unclassified_mails(
    db: State<DbState>,
    account_id: String,
) -> Result<Vec<Mail>, AppError> {
    db.with_conn(|conn| assignments::get_unclassified_mails(conn, &account_id))
}

/// 未分類メールをスレッド単位で返す（未分類一覧のスレッド表示用）。
/// 分類の実体はメール単位のまま（スレッドのD&Dは全メールIDを渡す）。
///
/// 取得の前にスレッド追従の自動分類（`auto_follow_threads`）を行う。
/// 同一スレッドの既存メールが単一の案件に割り当て済みなら、後から届いた返信等の
/// 未分類メールをその案件へ自動追従させる。一覧を開くたびに再計算する
/// （設計: docs/superpowers/specs/2026-07-13-thread-follow-classify-design.md）
#[tauri::command]
pub fn get_unclassified_threads(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    account_id: String,
) -> Result<Vec<crate::models::mail::Thread>, AppError> {
    db.with_conn(|conn| get_unclassified_threads_inner(conn, &pending, &account_id))
}

fn get_unclassified_threads_inner(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    account_id: &str,
) -> Result<Vec<crate::models::mail::Thread>, AppError> {
    let followed = assignments::auto_follow_threads(conn, account_id)?;
    // スレッド追従で割り当てが確定したメールの提案は不要になる
    for mail_id in &followed {
        pending.remove(mail_id)?;
    }
    let mails = assignments::get_unclassified_mails(conn, account_id)?;
    Ok(crate::db::mails::build_threads(&mails))
}

/// Move a mail to a different project (used by D&D and context menu).
#[tauri::command]
pub fn move_mail(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_id: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| move_mail_inner(conn, &pending, &mail_id, &project_id))
}

/// `move_mail` の本体。`bulk_move_mails` からも1件ずつ再利用される。
pub(crate) fn move_mail_inner(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    assignments::move_mail_to_project(conn, mail_id, project_id)?;
    // 割り当てが確定したので、残っている提案があれば除去する
    pending.remove(mail_id)
}

/// Get all mails assigned to a specific project.
#[tauri::command]
pub fn get_mails_by_project(db: State<DbState>, project_id: String) -> Result<Vec<Mail>, AppError> {
    db.with_conn(|conn| assignments::get_mails_by_project(conn, &project_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::classifier::{ClassifyAction, ClassifyResult};
    use crate::test_helpers::{insert_test_mail, setup_db};

    // --- get_mail_by_id (now in db::mails) ---

    #[test]
    fn test_get_mail_by_id_success() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Test Subject");
        let mail = mails::get_mail_by_id(&conn, "m1").unwrap();
        assert_eq!(mail.id, "m1");
        assert_eq!(mail.subject, "Test Subject");
    }

    #[test]
    fn test_get_mail_by_id_not_found() {
        let conn = setup_db();
        let result = mails::get_mail_by_id(&conn, "nonexistent");
        assert!(result.is_err());
    }

    // --- build_project_summaries ---

    #[test]
    fn test_build_project_summaries_empty() {
        let conn = setup_db();
        let summaries = projects::build_project_summaries(&conn, "acc1", false).unwrap();
        assert!(summaries.is_empty());
    }

    #[test]
    fn test_build_project_summaries_with_projects() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project Alpha".into(),
            description: Some("First project".into()),
            color: None,
        };
        projects::insert_project(&conn, &req).unwrap();
        let summaries = projects::build_project_summaries(&conn, "acc1", false).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "Project Alpha");
    }

    // --- approve_new_project flow ---

    #[test]
    fn test_approve_new_project_creates_project_and_assigns_mail() {
        let mut conn = setup_db();
        insert_test_mail(&conn, "m1", "New Deal");

        // Simulate what approve_new_project does
        let mail = mails::get_mail_by_id(&conn, "m1").unwrap();
        let tx = conn.transaction().unwrap();
        let req = CreateProjectRequest {
            account_id: mail.account_id.clone(),
            name: "New Project".into(),
            description: Some("Auto-created".into()),
            color: None,
        };
        let project = projects::insert_project(&tx, &req).unwrap();
        assignments::assign_mail(&tx, "m1", &project.id, "user", Some(1.0)).unwrap();
        tx.commit().unwrap();

        // Verify project was created
        let projs = projects::list_projects(&conn, "acc1").unwrap();
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].name, "New Project");

        // Verify mail was assigned
        let assigned_mails = assignments::get_mails_by_project(&conn, &projs[0].id).unwrap();
        assert_eq!(assigned_mails.len(), 1);
        assert_eq!(assigned_mails[0].id, "m1");
    }

    #[test]
    fn test_approve_new_project_transaction_rollback_on_error() {
        let mut conn = setup_db();
        // Don't insert mail — assign_mail will still succeed (no FK on mail_id in some schemas)
        // but we can test that transaction rolls back if we manually drop it
        let tx = conn.transaction().unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Will Rollback".into(),
            description: None,
            color: None,
        };
        let _project = projects::insert_project(&tx, &req).unwrap();
        // Drop tx without committing — should rollback
        drop(tx);

        let projs = projects::list_projects(&conn, "acc1").unwrap();
        assert!(projs.is_empty(), "Transaction should have been rolled back");
    }

    // --- reject_classification flow ---

    #[test]
    fn test_reject_removes_from_pending_map() {
        let pending = PendingClassifications::new();
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Test".into(),
                description: "desc".into(),
            },
            confidence: 0.8,
            reason: "test".into(),
        };
        pending.insert("m1".into(), result).unwrap();

        // Remove from pending
        pending.remove("m1").unwrap();
        assert!(!pending.contains("m1").unwrap());
    }

    // --- get_unclassified_mails flow ---

    #[test]
    fn test_get_unclassified_mails_returns_unassigned() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Unassigned Mail");
        insert_test_mail(&conn, "m2", "Also Unassigned");

        let unclassified = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 2);
    }

    #[test]
    fn test_get_unclassified_mails_excludes_sent_and_archive() {
        // 自分の送信済み・アーカイブ済みは分類対象にしない（INBOXのみ）
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Inbox Mail");
        let mut sent = crate::test_helpers::make_mail(
            "m2",
            "<sent@pigeon.local>",
            "Re: Inbox Mail",
            "2026-07-12T10:00:00",
        );
        sent.folder = "Sent".into();
        crate::db::mails::insert_mail(&conn, &sent).unwrap();
        let mut archived = crate::test_helpers::make_mail(
            "m3",
            "<archived@ex.com>",
            "Old Mail",
            "2026-07-11T10:00:00",
        );
        archived.folder = "Archive".into();
        crate::db::mails::insert_mail(&conn, &archived).unwrap();

        let unclassified = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
        assert_eq!(unclassified[0].id, "m1");
    }

    #[test]
    fn test_get_unclassified_threads_auto_follows_before_listing() {
        // 既に割り当て済みの m1 に対する返信 m2 は、一覧取得時に自動追従され
        // 未分類一覧から消えること
        let conn = setup_db();
        let m1 = crate::test_helpers::make_mail(
            "m1",
            "<m1@ex.com>",
            "Re: Test",
            "2026-07-12T10:00:00",
        );
        let mut m2 = crate::test_helpers::make_mail(
            "m2",
            "<m2@ex.com>",
            "Re: Test",
            "2026-07-12T11:00:00",
        );
        m2.in_reply_to = Some("<m1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "user", Some(1.0)).unwrap();

        let mails = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        let threads_before = crate::db::mails::build_threads(&mails);
        assert_eq!(threads_before.len(), 1, "m2は追従前は未分類一覧に見える");

        // get_unclassified_threads相当の処理（auto_follow_threads → 再取得）を直接呼ぶ
        assignments::auto_follow_threads(&conn, "acc1").unwrap();
        let mails_after = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert!(mails_after.is_empty(), "追従によりm2も未分類一覧から消える");
    }

    #[test]
    fn test_get_unclassified_threads_groups_replies() {
        // 返信の連鎖（References）が1スレッドにまとまること
        let conn = setup_db();
        let m1 = crate::test_helpers::make_mail(
            "m1",
            "<t1@ex.com>",
            "Re: Test",
            "2026-07-12T10:00:00",
        );
        let mut m2 = crate::test_helpers::make_mail(
            "m2",
            "<t3@ex.com>",
            "Re: Test",
            "2026-07-12T11:00:00",
        );
        // 中間のメール（自分の返信 <t2> は Sent にあり未分類一覧には無い）を
        // 飛び越えて References で繋がるケース
        m2.references = Some("<t1@ex.com> <t2@pigeon.local>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        insert_test_mail(&conn, "m3", "Unrelated");

        let mails = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        let threads = crate::db::mails::build_threads(&mails);
        assert_eq!(threads.len(), 2);
        let re_test = threads.iter().find(|t| t.mail_count == 2).unwrap();
        assert_eq!(re_test.mails.len(), 2);
    }

    #[test]
    fn test_get_unclassified_mails_excludes_assigned() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Assigned Mail");
        insert_test_mail(&conn, "m2", "Unassigned Mail");

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();

        let unclassified = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
        assert_eq!(unclassified[0].id, "m2");
    }

    // --- move_mail flow ---

    #[test]
    fn test_move_mail_between_projects() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Moving Mail");

        let req1 = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project A".into(),
            description: None,
            color: None,
        };
        let req2 = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project B".into(),
            description: None,
            color: None,
        };
        let proj_a = projects::insert_project(&conn, &req1).unwrap();
        let proj_b = projects::insert_project(&conn, &req2).unwrap();

        assignments::assign_mail(&conn, "m1", &proj_a.id, "ai", Some(0.9)).unwrap();
        assignments::move_mail_to_project(&conn, "m1", &proj_b.id).unwrap();

        let mails_a = assignments::get_mails_by_project(&conn, &proj_a.id).unwrap();
        let mails_b = assignments::get_mails_by_project(&conn, &proj_b.id).unwrap();
        assert!(mails_a.is_empty());
        assert_eq!(mails_b.len(), 1);
        assert_eq!(mails_b[0].id, "m1");
    }

    // --- pending リーク防止: 割り当てが確定する全経路で提案が除去される ---

    fn pending_create_result() -> ClassifyResult {
        ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Suggested".into(),
                description: "desc".into(),
            },
            confidence: 0.8,
            reason: "test".into(),
        }
    }

    fn insert_project_for(conn: &rusqlite::Connection, name: &str) -> Project {
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: name.into(),
            description: None,
            color: None,
        };
        projects::insert_project(conn, &req).unwrap()
    }

    #[test]
    fn test_move_mail_removes_pending_entry() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project_for(&conn, "Proj");
        pending
            .insert("m1".into(), pending_create_result())
            .unwrap();

        move_mail_inner(&conn, &pending, "m1", &proj.id).unwrap();

        assert!(
            !pending.contains("m1").unwrap(),
            "手動割り当てで確定したら提案は残らない"
        );
    }

    #[test]
    fn test_move_mail_failure_keeps_pending_entry() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        // メールがDBに無いので move は失敗する
        pending
            .insert("ghost".into(), pending_create_result())
            .unwrap();

        let result = move_mail_inner(&conn, &pending, "ghost", "proj-x");

        assert!(result.is_err());
        assert!(
            pending.contains("ghost").unwrap(),
            "割り当てが確定していないときは提案を保持する"
        );
    }

    #[test]
    fn test_approve_classification_removes_pending_entry() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project_for(&conn, "Proj");
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();
        pending
            .insert("m1".into(), pending_create_result())
            .unwrap();

        approve_classification_inner(&conn, &pending, "m1", &proj.id).unwrap();

        assert!(
            !pending.contains("m1").unwrap(),
            "承認で確定したら提案は残らない"
        );
    }

    #[test]
    fn test_get_unclassified_threads_inner_removes_followed_pending() {
        // スレッド追従で割り当てが確定したメールの Create 提案も除去される
        let conn = setup_db();
        let pending = PendingClassifications::new();
        let m1 =
            crate::test_helpers::make_mail("m1", "<m1@ex.com>", "Re: Test", "2026-07-12T10:00:00");
        let mut m2 =
            crate::test_helpers::make_mail("m2", "<m2@ex.com>", "Re: Test", "2026-07-12T11:00:00");
        m2.in_reply_to = Some("<m1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        let proj = insert_project_for(&conn, "Proj");
        assignments::assign_mail(&conn, "m1", &proj.id, "user", Some(1.0)).unwrap();
        pending
            .insert("m2".into(), pending_create_result())
            .unwrap();

        let threads = get_unclassified_threads_inner(&conn, &pending, "acc1").unwrap();

        assert!(threads.is_empty(), "m2 は追従割り当てされ一覧から消える");
        assert!(
            !pending.contains("m2").unwrap(),
            "追従で確定したメールの提案も除去される"
        );
    }

}
