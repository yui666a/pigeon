use crate::db::project_notes;
use crate::error::AppError;
use crate::models::project_note::{AiHistoryEntry, ProjectNote};
use crate::project_notes_sync;
use crate::state::DbState;
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct GenerateNoteOutcome {
    pub ai_md: String,
    /// 上限超過で AI 入力から除外したメール件数（0 なら全件使用）
    pub dropped_mails: usize,
}

/// 「ノート」タブ保存の実体。保存 → キャッシュ再生成 → ディスク同期。
/// ディスク書き込み失敗は DB 正本を巻き戻さず警告に留める（設計書 §7）。
fn save_user_note_inner(
    conn: &Connection,
    project_id: &str,
    user_md: &str,
) -> Result<(), AppError> {
    project_notes::upsert_user_md(conn, project_id, user_md)?;
    project_notes_sync::refresh_cached_context(conn, project_id)?;
    if let Err(e) = project_notes_sync::sync_note_to_disk(conn, project_id) {
        eprintln!("[warn] PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
    }
    Ok(())
}

/// 「AI要約」タブの手編集保存の実体。ai_edited を立てる。
fn save_ai_note_inner(conn: &Connection, project_id: &str, ai_md: &str) -> Result<(), AppError> {
    project_notes::upsert_ai_md(conn, project_id, ai_md, true)?;
    project_notes_sync::refresh_cached_context(conn, project_id)?;
    if let Err(e) = project_notes_sync::sync_note_to_disk(conn, project_id) {
        eprintln!("[warn] PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
    }
    Ok(())
}

#[tauri::command]
pub fn get_project_note(
    db: State<DbState>,
    project_id: String,
) -> Result<Option<ProjectNote>, AppError> {
    db.with_conn(|conn| project_notes::get_note(conn, &project_id))
}

#[tauri::command]
pub fn save_project_note_user(
    db: State<DbState>,
    project_id: String,
    user_md: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| save_user_note_inner(conn, &project_id, &user_md))
}

#[tauri::command]
pub fn save_project_note_ai(
    db: State<DbState>,
    project_id: String,
    ai_md: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| save_ai_note_inner(conn, &project_id, &ai_md))
}

/// 案件所属メールから AI 要約を生成し、既存要約を履歴へ退避して差し替える。
/// メール0件の場合は生成せずエラーを返す。クラウド送信可否はフロントから
/// 受け取らず `is_cloud_provider_configured` でサーバー側から導出する
/// （`rescan_project_directory` と同じ方針）。
#[tauri::command]
pub async fn generate_project_note_ai(
    db: State<'_, DbState>,
    secure_store: State<'_, crate::state::SecureStoreState>,
    project_id: String,
) -> Result<GenerateNoteOutcome, AppError> {
    // SecureStore の解決は DB ロックを取る前に済ませる（ADR 0006 決定 3）
    let secure_store = secure_store.get()?;
    // 1. スナップショット取得 + 生成器構築（ロック内）
    let (classifier, project_name, mails) = db.with_conn(|conn| {
        let classifier = crate::classifier::factory::build_classifier(conn, secure_store)?;
        let project = crate::db::projects::get_project(conn, &project_id)?;
        let mails = crate::db::assignments::get_mails_by_project(conn, &project_id)?;
        Ok((classifier, project.name, mails))
    })?;

    if mails.is_empty() {
        return Err(AppError::Validation(
            "この案件にはメールがないため要約を生成できません".into(),
        ));
    }

    // 2. 入力組み立て + LLM 呼び出し（ロック外）
    let (input, dropped) =
        crate::project_note_digest::build_mail_digest_input(&project_name, &mails);
    if dropped > 0 {
        eprintln!(
            "[info] 案件 {} のAI要約: メール {} 件中 {} 件を上限超過で除外",
            project_id,
            mails.len(),
            dropped
        );
    }
    let ai_md =
        crate::project_note_digest::generate_mail_digest(classifier.as_ref(), &input).await?;

    // 3. 書き込み（ロック内）。既存要約は履歴へ退避される
    db.with_conn_mut(|conn| {
        project_notes::replace_ai_md_with_history(conn, &project_id, &ai_md)?;
        project_notes_sync::refresh_cached_context(conn, &project_id)?;
        if let Err(e) = project_notes_sync::sync_note_to_disk(conn, &project_id) {
            eprintln!("[warn] PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
        }
        Ok(())
    })?;

    Ok(GenerateNoteOutcome {
        ai_md,
        dropped_mails: dropped,
    })
}

#[tauri::command]
pub fn list_project_note_ai_history(
    db: State<DbState>,
    project_id: String,
) -> Result<Vec<AiHistoryEntry>, AppError> {
    db.with_conn(|conn| project_notes::list_ai_history(conn, &project_id))
}

#[tauri::command]
pub fn restore_project_note_ai(db: State<DbState>, history_id: String) -> Result<(), AppError> {
    db.with_conn_mut(|conn| project_notes::restore_ai_from_history(conn, &history_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_save_user_note_refreshes_cached_context() {
        let conn = setup_db();
        create_project(&conn);

        save_user_note_inner(&conn, "p1", "会場は〇〇ホール").unwrap();

        let note = project_notes::get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.user_md, "会場は〇〇ホール");

        // 分類プロンプト用キャッシュも更新される
        let ctx = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(ctx.cached_context.unwrap().contains("会場は〇〇ホール"));
    }

    #[test]
    fn test_save_ai_note_marks_edited() {
        let conn = setup_db();
        create_project(&conn);
        save_ai_note_inner(&conn, "p1", "手で直したAI要約").unwrap();
        let note = project_notes::get_note(&conn, "p1").unwrap().unwrap();
        assert!(note.ai_edited, "手編集保存は ai_edited を立てる");
    }
}
