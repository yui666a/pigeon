use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

use crate::classifier::ollama::OllamaClassifier;
use crate::classifier::LlmClassifier;
use crate::commands::account_commands::DbState;
use crate::db::{assignments, projects};
use crate::models::classifier::{
    ClassifyAction, ClassifyResponse, ClassifyResult, MailSummary, ProjectSummary,
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
// Private helpers
// ---------------------------------------------------------------------------

/// Query the settings table for `key`, returning `default` if the row doesn't exist.
fn get_settings_or_default(conn: &rusqlite::Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| default.to_string())
}

/// Load all non-archived projects for `account_id` and attach their recent subjects.
fn build_project_summaries(
    conn: &rusqlite::Connection,
    account_id: &str,
) -> Result<Vec<ProjectSummary>, String> {
    let projs = projects::list_projects(conn, account_id).map_err(|e| e.to_string())?;
    let mut summaries = Vec::with_capacity(projs.len());
    for p in projs {
        let recent_subjects =
            assignments::get_recent_subjects(conn, &p.id, 5).unwrap_or_default();
        summaries.push(ProjectSummary {
            id: p.id,
            name: p.name,
            description: p.description,
            recent_subjects,
        });
    }
    Ok(summaries)
}

/// Load a single mail by ID from the database.
fn load_mail(conn: &rusqlite::Connection, mail_id: &str) -> Result<Mail, String> {
    conn.query_row(
        "SELECT id, account_id, folder, message_id, in_reply_to, \"references\",
                from_addr, to_addr, cc_addr, subject, body_text, body_html,
                date, has_attachments, raw_size, uid, flags, fetched_at
         FROM mails WHERE id = ?1",
        rusqlite::params![mail_id],
        |row| {
            Ok(Mail {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder: row.get(2)?,
                message_id: row.get(3)?,
                in_reply_to: row.get(4)?,
                references: row.get(5)?,
                from_addr: row.get(6)?,
                to_addr: row.get(7)?,
                cc_addr: row.get(8)?,
                subject: row.get(9)?,
                body_text: row.get(10)?,
                body_html: row.get(11)?,
                date: row.get(12)?,
                has_attachments: row.get(13)?,
                raw_size: row.get(14)?,
                uid: row.get(15)?,
                flags: row.get(16)?,
                fetched_at: row.get(17)?,
            })
        },
    )
    .map_err(|_| format!("Mail not found: {}", mail_id))
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
    let (mail, project_summaries, endpoint, model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mail = load_mail(&conn, &mail_id)?;
        let project_summaries = build_project_summaries(&conn, &mail.account_id)?;
        let endpoint = get_settings_or_default(&conn, "ollama_endpoint", "http://localhost:11434");
        let model = get_settings_or_default(&conn, "ollama_model", "llama3.1:8b");
        (mail, project_summaries, endpoint, model)
    };

    let mail_summary = MailSummary::from_mail(&mail);
    let classifier = OllamaClassifier::new(&endpoint, &model);

    // Health check
    classifier
        .health_check()
        .await
        .map_err(|e| e.to_string())?;

    // Classify
    let result = classifier
        .classify(&mail_summary, &project_summaries, &[])
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
    eprintln!("[classify] classify_unassigned called for account {}", account_id);

    // Reset cancel flag
    cancel_flag.0.store(false, Ordering::SeqCst);

    // Load unclassified mails and settings
    let (mails, endpoint, model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mails = assignments::get_unclassified_mails(&conn, &account_id)
            .map_err(|e| e.to_string())?;
        let endpoint = get_settings_or_default(&conn, "ollama_endpoint", "http://localhost:11434");
        let model = get_settings_or_default(&conn, "ollama_model", "llama3.1:8b");
        (mails, endpoint, model)
    };

    eprintln!("[classify] found {} unclassified mails, using model {} at {}", mails.len(), model, endpoint);

    let classifier = OllamaClassifier::new(&endpoint, &model);

    // Health check before starting the loop
    eprintln!("[classify] running health check...");
    classifier
        .health_check()
        .await
        .map_err(|e| {
            eprintln!("[classify] health check failed: {}", e);
            e.to_string()
        })?;
    eprintln!("[classify] health check passed");

    let total = mails.len();
    let mut assigned = 0u32;
    let mut needs_review = 0u32;
    let mut unclassified_count = 0u32;

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

        // Load project summaries fresh for each mail (projects may have been created)
        let project_summaries = {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            build_project_summaries(&conn, &account_id)?
        };

        let mail_summary = MailSummary::from_mail(mail);
        eprintln!("[classify] classifying mail {}/{}: {}", idx + 1, total, mail_summary.subject);

        let result = match classifier
            .classify(&mail_summary, &project_summaries, &[])
            .await
        {
            Ok(r) => {
                eprintln!("[classify] result: confidence={}", r.confidence);
                r
            }
            Err(e) => {
                eprintln!("[classify] error: {}", e);
                ClassifyResult {
                    action: ClassifyAction::Unclassified,
                    confidence: 0.0,
                    reason: "分類中にエラーが発生しました".to_string(),
                }
            }
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
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Load mail to get account_id
    let mail = load_mail(&conn, &mail_id)?;

    // Wrap in transaction
    conn.execute("BEGIN", []).map_err(|e| e.to_string())?;

    let result: Result<Project, String> = (|| {
        let req = CreateProjectRequest {
            account_id: mail.account_id.clone(),
            name: project_name,
            description,
            color: None,
        };
        let project = projects::insert_project(&conn, &req).map_err(|e| e.to_string())?;

        assignments::assign_mail(&conn, &mail_id, &project.id, "user", Some(1.0))
            .map_err(|e| e.to_string())?;

        Ok(project)
    })();

    match result {
        Ok(project) => {
            conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
            // Remove from pending map
            let mut map = pending.0.lock().map_err(|e| e.to_string())?;
            map.remove(&mail_id);
            Ok(project)
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
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

/// Get all mails assigned to a specific project.
#[tauri::command]
pub fn get_mails_by_project(
    db: State<DbState>,
    project_id: String,
) -> Result<Vec<Mail>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::get_mails_by_project(&conn, &project_id).map_err(|e| e.to_string())
}
