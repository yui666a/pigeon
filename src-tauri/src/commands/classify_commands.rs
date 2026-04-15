use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

use crate::classifier::ollama::OllamaClassifier;
use crate::classifier::LlmClassifier;
use crate::state::DbState;
use crate::db::{assignments, mails, projects, settings};
use crate::models::classifier::{
    ClassifyAction, ClassifyResponse, ClassifyResult, MailSummary,
    CONFIDENCE_UNCERTAIN,
};
use crate::models::mail::Mail;
use crate::models::project::{CreateProjectRequest, Project};

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

pub struct PendingClassifications(pub Mutex<HashMap<String, ClassifyResult>>);

impl PendingClassifications {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

pub struct ClassifyCancelFlag(pub Arc<AtomicBool>);

impl ClassifyCancelFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
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
    mail_id: String,
) -> Result<ClassifyResponse, String> {
    // Load mail and settings while holding the lock briefly.
    let (mail, project_summaries, corrections, endpoint, model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mail = mails::get_mail_by_id(&conn, &mail_id).map_err(|e| e.to_string())?;
        let project_summaries = projects::build_project_summaries(&conn, &mail.account_id)
            .map_err(|e| e.to_string())?;
        let corrections = assignments::get_recent_corrections(&conn, &mail.account_id, 20)
            .unwrap_or_default();
        let endpoint = settings::get_or_default(&conn,"ollama_endpoint", "http://localhost:11434");
        let model = settings::get_or_default(&conn,"ollama_model", "llama3.1:8b");
        (mail, project_summaries, corrections, endpoint, model)
    };

    let mail_summary = MailSummary::from_mail(&mail);
    let classifier = OllamaClassifier::new(&endpoint, &model).map_err(|e| e.to_string())?;

    // Health check
    classifier
        .health_check()
        .await
        .map_err(|e| e.to_string())?;

    // Classify
    let result = classifier
        .classify(&mail_summary, &project_summaries, &corrections)
        .await
        .map_err(|e| e.to_string())?;

    // Persist / queue pending
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        match &result.action {
            ClassifyAction::Assign { project_id } if result.confidence >= CONFIDENCE_UNCERTAIN => {
                assignments::assign_mail(
                    &conn,
                    &mail_id,
                    project_id,
                    "ai",
                    Some(result.confidence),
                )
                .map_err(|e| e.to_string())?;
            }
            ClassifyAction::Create { .. } => {
                let mut map = pending.0.lock().map_err(|e| e.to_string())?;
                map.insert(mail_id.clone(), result.clone());
            }
            _ => {}
        }
    }

    Ok(ClassifyResponse {
        mail_id,
        result,
    })
}

/// Classify all unassigned mails for `account_id`, emitting progress events.
#[tauri::command]
pub async fn classify_unassigned(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    cancel_flag: State<'_, ClassifyCancelFlag>,
    handle: AppHandle,
    account_id: String,
) -> Result<(), String> {
    // Reset cancel flag
    cancel_flag.0.store(false, Ordering::SeqCst);

    // Load unclassified mails and settings
    let (mails, corrections, endpoint, model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mails = assignments::get_unclassified_mails(&conn, &account_id)
            .map_err(|e| e.to_string())?;
        let corrections = assignments::get_recent_corrections(&conn, &account_id, 20)
            .unwrap_or_default();
        let endpoint = settings::get_or_default(&conn,"ollama_endpoint", "http://localhost:11434");
        let model = settings::get_or_default(&conn,"ollama_model", "llama3.1:8b");
        (mails, corrections, endpoint, model)
    };

    let classifier = OllamaClassifier::new(&endpoint, &model).map_err(|e| e.to_string())?;

    // Health check before starting the loop
    classifier
        .health_check()
        .await
        .map_err(|e| e.to_string())?;

    let total = mails.len();
    let mut assigned = 0u32;
    let mut needs_review = 0u32;
    let mut unclassified_count = 0u32;

    // Load project summaries once before the loop.
    // New projects are only inserted when the user approves (approve_new_project),
    // not during classification, so per-iteration reload is unnecessary.
    let project_summaries = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        projects::build_project_summaries(&conn, &account_id)
            .map_err(|e| e.to_string())?
    };

    for (idx, mail) in mails.iter().enumerate() {
        // Check cancellation
        if cancel_flag.0.load(Ordering::SeqCst) {
            let _ = handle.emit(
                "classify-progress",
                serde_json::json!({
                    "current": idx,
                    "total": total,
                    "cancelled": true,
                }),
            );
            return Ok(());
        }

        let mail_summary = MailSummary::from_mail(mail);

        let result = match classifier
            .classify(&mail_summary, &project_summaries, &corrections)
            .await
        {
            Ok(r) => r,
            Err(_) => ClassifyResult {
                action: ClassifyAction::Unclassified,
                confidence: 0.0,
                reason: "分類中にエラーが発生しました".to_string(),
            },
        };

        let response = ClassifyResponse {
            mail_id: mail.id.clone(),
            result: result.clone(),
        };

        // Persist result
        {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            match &result.action {
                ClassifyAction::Assign { project_id }
                    if result.confidence >= CONFIDENCE_UNCERTAIN =>
                {
                    let _ = assignments::assign_mail(
                        &conn,
                        &mail.id,
                        project_id,
                        "ai",
                        Some(result.confidence),
                    );
                    assigned += 1;
                }
                ClassifyAction::Create { .. } => {
                    let mut map = pending.0.lock().map_err(|e| e.to_string())?;
                    map.insert(mail.id.clone(), result);
                    needs_review += 1;
                }
                _ => {
                    unclassified_count += 1;
                }
            }
        }

        // Emit progress with result
        let _ = handle.emit(
            "classify-progress",
            serde_json::json!({
                "current": idx,
                "total": total,
                "result": response,
            }),
        );
    }

    // Emit completion event
    let _ = handle.emit(
        "classify-complete",
        serde_json::json!({
            "total": total,
            "assigned": assigned,
            "needs_review": needs_review,
            "unclassified": unclassified_count,
        }),
    );

    Ok(())
}

/// Cancel an in-progress `classify_unassigned` run.
#[tauri::command]
pub fn cancel_classification(cancel_flag: State<ClassifyCancelFlag>) -> Result<(), String> {
    cancel_flag.0.store(true, Ordering::SeqCst);
    Ok(())
}

/// Approve an AI classification (user confirms the assigned project).
#[tauri::command]
pub fn approve_classification(
    db: State<DbState>,
    mail_id: String,
    project_id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::approve_classification(&conn, &mail_id, &project_id)
        .map_err(|e| e.to_string())
}

/// Approve a "create new project" suggestion: creates the project and assigns the mail.
#[tauri::command]
pub fn approve_new_project(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_name: String,
    description: Option<String>,
) -> Result<Project, String> {
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;

    // Load mail to get account_id
    let mail = mails::get_mail_by_id(&conn, &mail_id).map_err(|e| e.to_string())?;

    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let req = CreateProjectRequest {
        account_id: mail.account_id.clone(),
        name: project_name,
        description,
        color: None,
    };
    let project = projects::insert_project(&tx, &req).map_err(|e| e.to_string())?;

    assignments::assign_mail(&tx, &mail_id, &project.id, "user", Some(1.0))
        .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;

    // Remove from pending map
    let mut map = pending.0.lock().map_err(|e| e.to_string())?;
    map.remove(&mail_id);
    Ok(project)
}

/// Reject an AI classification (remove from pending or delete assignment).
#[tauri::command]
pub fn reject_classification(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
) -> Result<(), String> {
    // Remove from pending map if present
    {
        let mut map = pending.0.lock().map_err(|e| e.to_string())?;
        map.remove(&mail_id);
    }

    // Also remove from DB assignments if present
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let result = assignments::reject_classification(&conn, &mail_id);
    match result {
        Ok(()) => Ok(()),
        // MailNotFound means there was no assignment — that's fine after removing from pending
        Err(crate::error::AppError::MailNotFound(_)) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Get all mails that have not yet been assigned to a project.
#[tauri::command]
pub fn get_unclassified_mails(
    db: State<DbState>,
    account_id: String,
) -> Result<Vec<Mail>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::get_unclassified_mails(&conn, &account_id).map_err(|e| e.to_string())
}

/// Move a mail to a different project (used by D&D and context menu).
#[tauri::command]
pub fn move_mail(
    db: State<DbState>,
    mail_id: String,
    project_id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::move_mail_to_project(&conn, &mail_id, &project_id)
        .map_err(|e| e.to_string())
}

/// Get all mails assigned to a specific project.
#[tauri::command]
pub fn get_mails_by_project(
    db: State<DbState>,
    project_id: String,
) -> Result<Vec<Mail>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::get_mails_by_project(&conn, &project_id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use crate::db::mails;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        ).unwrap();
        conn
    }

    fn insert_test_mail(conn: &Connection, id: &str, subject: &str) {
        let mail = Mail {
            id: id.into(),
            account_id: "acc1".into(),
            folder: "INBOX".into(),
            message_id: format!("<{}@test.com>", id),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "me@example.com".into(),
            cc_addr: None,
            subject: subject.into(),
            body_text: Some("Hello".into()),
            body_html: None,
            date: "2026-04-13T10:00:00".into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        };
        mails::insert_mail(conn, &mail).unwrap();
    }

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
        let summaries = projects::build_project_summaries(&conn, "acc1").unwrap();
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
        let summaries = projects::build_project_summaries(&conn, "acc1").unwrap();
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

    // --- cancel flag ---

    #[test]
    fn test_cancel_flag_toggle() {
        let flag = ClassifyCancelFlag::new();
        assert!(!flag.0.load(Ordering::SeqCst));
        flag.0.store(true, Ordering::SeqCst);
        assert!(flag.0.load(Ordering::SeqCst));
        flag.0.store(false, Ordering::SeqCst);
        assert!(!flag.0.load(Ordering::SeqCst));
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
