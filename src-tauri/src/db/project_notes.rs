use crate::error::AppError;
use crate::models::project_note::{AiHistoryEntry, ProjectNote};
use rusqlite::{params, Connection, OptionalExtension};

/// AI要約履歴の保持上限。これを超えた古い履歴は退避時に削除する。
pub const AI_HISTORY_LIMIT: usize = 10;

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectNote> {
    Ok(ProjectNote {
        project_id: row.get(0)?,
        user_md: row.get(1)?,
        ai_md: row.get(2)?,
        ai_edited: row.get(3)?,
        ai_generated_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub fn get_note(conn: &Connection, project_id: &str) -> Result<Option<ProjectNote>, AppError> {
    conn.query_row(
        "SELECT project_id, user_md, ai_md, ai_edited, ai_generated_at, updated_at
         FROM project_notes WHERE project_id = ?1",
        params![project_id],
        row_to_note,
    )
    .optional()
    .map_err(AppError::Database)
}

/// 「ノート」タブの保存。ai_md 側は触らない。
pub fn upsert_user_md(conn: &Connection, project_id: &str, user_md: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_notes (project_id, user_md, updated_at)
         VALUES (?1, ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            user_md = ?2, updated_at = CURRENT_TIMESTAMP",
        params![project_id, user_md],
    )?;
    Ok(())
}

/// 「AI要約」タブの保存。mark_edited=true はユーザー手編集を意味する。
pub fn upsert_ai_md(
    conn: &Connection,
    project_id: &str,
    ai_md: &str,
    mark_edited: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_notes (project_id, ai_md, ai_edited, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            ai_md = ?2, ai_edited = ?3, updated_at = CURRENT_TIMESTAMP",
        params![project_id, ai_md, mark_edited],
    )?;
    Ok(())
}

/// AI再生成。既存 ai_md があれば履歴へ退避してから上書きし、履歴を上限まで剪定する。
/// 退避と上書きは1トランザクションで行う（片方だけ成功する状態を作らない）。
pub fn replace_ai_md_with_history(
    conn: &mut Connection,
    project_id: &str,
    new_ai_md: &str,
) -> Result<(), AppError> {
    let tx = conn.transaction()?;

    let existing: Option<String> = tx
        .query_row(
            "SELECT ai_md FROM project_notes WHERE project_id = ?1",
            params![project_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(AppError::Database)?
        .flatten();

    if let Some(old) = existing {
        if !old.is_empty() {
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO project_note_ai_history (id, project_id, ai_md)
                 VALUES (?1, ?2, ?3)",
                params![id, project_id, old],
            )?;
            // 上限を超えた古い履歴を削除
            tx.execute(
                "DELETE FROM project_note_ai_history
                 WHERE project_id = ?1 AND id NOT IN (
                     SELECT id FROM project_note_ai_history
                     WHERE project_id = ?1
                     ORDER BY replaced_at DESC, rowid DESC
                     LIMIT ?2
                 )",
                params![project_id, AI_HISTORY_LIMIT as i64],
            )?;
        }
    }

    tx.execute(
        "INSERT INTO project_notes
            (project_id, ai_md, ai_edited, ai_generated_at, updated_at)
         VALUES (?1, ?2, FALSE, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            ai_md = ?2, ai_edited = FALSE,
            ai_generated_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP",
        params![project_id, new_ai_md],
    )?;

    tx.commit()?;
    Ok(())
}

pub fn list_ai_history(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<AiHistoryEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, ai_md, replaced_at
         FROM project_note_ai_history
         WHERE project_id = ?1
         ORDER BY replaced_at DESC, rowid DESC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(AiHistoryEntry {
            id: row.get(0)?,
            project_id: row.get(1)?,
            ai_md: row.get(2)?,
            replaced_at: row.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 履歴から ai_md を復元する。復元自体も再生成扱いで現在値を履歴へ退避する。
pub fn restore_ai_from_history(conn: &mut Connection, history_id: &str) -> Result<(), AppError> {
    let (project_id, ai_md): (String, String) = conn
        .query_row(
            "SELECT project_id, ai_md FROM project_note_ai_history WHERE id = ?1",
            params![history_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::Validation(format!("history not found: {}", history_id)))?;

    replace_ai_md_with_history(conn, &project_id, &ai_md)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_get_note_none_initially() {
        let conn = setup_db();
        create_project(&conn);
        assert!(get_note(&conn, "p1").unwrap().is_none());
    }

    #[test]
    fn test_upsert_user_md_creates_row() {
        let conn = setup_db();
        create_project(&conn);
        upsert_user_md(&conn, "p1", "# 会場メモ").unwrap();
        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.user_md, "# 会場メモ");
        assert_eq!(note.ai_md, None);
        assert!(!note.ai_edited);
    }

    #[test]
    fn test_upsert_user_md_preserves_ai_md() {
        let conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "AI要約", false).unwrap();
        upsert_user_md(&conn, "p1", "手書き").unwrap();
        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.user_md, "手書き");
        assert_eq!(note.ai_md.as_deref(), Some("AI要約"), "ai_md は消えない");
    }

    #[test]
    fn test_upsert_ai_md_marks_edited() {
        let conn = setup_db();
        create_project(&conn);
        // AI生成時は edited=false
        upsert_ai_md(&conn, "p1", "生成結果", false).unwrap();
        assert!(!get_note(&conn, "p1").unwrap().unwrap().ai_edited);
        // ユーザー手編集時は edited=true
        upsert_ai_md(&conn, "p1", "手で直した", true).unwrap();
        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert!(note.ai_edited);
        assert_eq!(note.ai_md.as_deref(), Some("手で直した"));
    }

    #[test]
    fn test_replace_ai_md_moves_old_to_history() {
        let mut conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "旧要約", true).unwrap();

        replace_ai_md_with_history(&mut conn, "p1", "新要約").unwrap();

        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.ai_md.as_deref(), Some("新要約"));
        assert!(!note.ai_edited, "再生成後は edited がリセットされる");
        assert!(note.ai_generated_at.is_some());

        let hist = list_ai_history(&conn, "p1").unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].ai_md, "旧要約");
    }

    #[test]
    fn test_replace_ai_md_no_history_when_empty() {
        let mut conn = setup_db();
        create_project(&conn);
        // 既存 ai_md が無い初回生成では履歴を作らない
        replace_ai_md_with_history(&mut conn, "p1", "初回").unwrap();
        assert!(list_ai_history(&conn, "p1").unwrap().is_empty());
    }

    #[test]
    fn test_history_pruned_to_limit() {
        let mut conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "v0", false).unwrap();
        // AI_HISTORY_LIMIT を超える回数だけ再生成する
        for i in 1..=(AI_HISTORY_LIMIT + 3) {
            replace_ai_md_with_history(&mut conn, "p1", &format!("v{}", i)).unwrap();
        }
        let hist = list_ai_history(&conn, "p1").unwrap();
        assert_eq!(hist.len(), AI_HISTORY_LIMIT, "上限を超えて溜まらない");
    }

    #[test]
    fn test_restore_ai_from_history() {
        let mut conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "旧要約", false).unwrap();
        replace_ai_md_with_history(&mut conn, "p1", "新要約").unwrap();

        let hist = list_ai_history(&conn, "p1").unwrap();
        let target = hist[0].id.clone();
        restore_ai_from_history(&mut conn, &target).unwrap();

        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.ai_md.as_deref(), Some("旧要約"), "履歴の内容が戻る");
    }

    #[test]
    fn test_restore_missing_history_errors() {
        let mut conn = setup_db();
        create_project(&conn);
        assert!(restore_ai_from_history(&mut conn, "nonexistent").is_err());
    }
}
