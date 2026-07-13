use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;

use crate::classifier::factory::build_classifier;
use crate::db::{assignments, mails, projects};
use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyResponse, ClassifyResult, MailSummary, CONFIDENCE_UNCERTAIN,
};
use crate::models::mail::Mail;
use crate::models::project::{CreateProjectRequest, Project};
use crate::state::{DbState, SecureStoreState};

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

pub struct PendingClassifications(pub Mutex<HashMap<String, ClassifyResult>>);

impl Default for PendingClassifications {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingClassifications {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Classify a single mail by ID and persist the result.
#[tauri::command]
pub async fn classify_mail(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    secure_store: State<'_, SecureStoreState>,
    mail_id: String,
) -> Result<ClassifyResponse, AppError> {
    // Load mail and settings while holding the lock briefly.
    let (mail, project_summaries, corrections, classifier) = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        let mail = mails::get_mail_by_id(&conn, &mail_id)?;
        let project_summaries = projects::build_project_summaries(&conn, &mail.account_id, false)?;
        let corrections = assignments::get_recent_corrections(&conn, &mail.account_id, 20)?;
        let classifier = build_classifier(&conn, &secure_store.0)?;
        (mail, project_summaries, corrections, classifier)
    };

    let mail_summary = MailSummary::from_mail(&mail);

    // Health check
    classifier.health_check().await?;

    // Classify
    let result = classifier
        .classify(&mail_summary, &project_summaries, &corrections)
        .await?;

    // Persist / queue pending
    {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        match &result.action {
            ClassifyAction::Assign { project_id } if result.confidence >= CONFIDENCE_UNCERTAIN => {
                assignments::assign_mail(
                    &conn,
                    &mail_id,
                    project_id,
                    "ai",
                    Some(result.confidence),
                )?;
            }
            ClassifyAction::Create { .. } => {
                let mut map = pending.0.lock().map_err(AppError::lock_err)?;
                map.insert(mail_id.clone(), result.clone());
            }
            _ => {}
        }
    }

    Ok(ClassifyResponse { mail_id, result })
}

/// Approve an AI classification (user confirms the assigned project).
#[tauri::command]
pub fn approve_classification(
    db: State<DbState>,
    mail_id: String,
    project_id: String,
) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    assignments::approve_classification(&conn, &mail_id, &project_id)
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
    let mut conn = db.0.lock().map_err(AppError::lock_err)?;

    // Load mail to get account_id
    let mail = mails::get_mail_by_id(&conn, &mail_id)?;

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

    // Remove from pending map
    let mut map = pending.0.lock().map_err(AppError::lock_err)?;
    map.remove(&mail_id);
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
    {
        let mut map = pending.0.lock().map_err(AppError::lock_err)?;
        map.remove(&mail_id);
    }

    // Also remove from DB assignments if present
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    let result = assignments::reject_classification(&conn, &mail_id);
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
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    assignments::get_unclassified_mails(&conn, &account_id)
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
    account_id: String,
) -> Result<Vec<crate::models::mail::Thread>, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    assignments::auto_follow_threads(&conn, &account_id)?;
    let mails = assignments::get_unclassified_mails(&conn, &account_id)?;
    Ok(crate::db::mails::build_threads(&mails))
}

/// Move a mail to a different project (used by D&D and context menu).
#[tauri::command]
pub fn move_mail(db: State<DbState>, mail_id: String, project_id: String) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    assignments::move_mail_to_project(&conn, &mail_id, &project_id)
}

/// Get all mails assigned to a specific project.
#[tauri::command]
pub fn get_mails_by_project(db: State<DbState>, project_id: String) -> Result<Vec<Mail>, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    assignments::get_mails_by_project(&conn, &project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        pending.0.lock().unwrap().insert("m1".into(), result);

        // Remove from pending
        pending.0.lock().unwrap().remove("m1");
        assert!(pending.0.lock().unwrap().get("m1").is_none());
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

    // --- PendingClassifications ---

    #[test]
    fn test_pending_classifications_insert_and_remove() {
        let pending = PendingClassifications::new();
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "New".into(),
                description: "desc".into(),
            },
            confidence: 0.75,
            reason: "reason".into(),
        };

        {
            let mut map = pending.0.lock().unwrap();
            map.insert("mail-1".into(), result.clone());
            map.insert("mail-2".into(), result);
        }

        {
            let map = pending.0.lock().unwrap();
            assert_eq!(map.len(), 2);
            assert!(map.contains_key("mail-1"));
        }

        {
            let mut map = pending.0.lock().unwrap();
            map.remove("mail-1");
            assert_eq!(map.len(), 1);
            assert!(!map.contains_key("mail-1"));
        }
    }
}
