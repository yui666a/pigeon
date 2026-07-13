use crate::error::AppError;
use crate::models::directory::ProjectDirectory;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

/// project_directories テーブルの1行を ProjectDirectory へ変換する共通マッパー。
/// カラム順は `SELECT_COLS` に一致させること。
fn row_to_directory(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectDirectory> {
    Ok(ProjectDirectory {
        id: row.get(0)?,
        project_id: row.get(1)?,
        path: row.get(2)?,
        is_primary: row.get(3)?,
        status: row.get(4)?,
        last_scanned_at: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const SELECT_COLS: &str = "id, project_id, path, is_primary, status, last_scanned_at, created_at";

/// 案件にディレクトリを紐付ける。既存の紐付けがあれば置換する。
/// DELETE+INSERT+SELECT をトランザクションで包み、INSERT が UNIQUE(path) 違反等で
/// 失敗した場合でも既存の紐付けが失われないようにする（db/project_files.rs の
/// replace_inventory と同様式）。
pub fn link_directory(
    conn: &mut Connection,
    project_id: &str,
    path: &str,
) -> Result<ProjectDirectory, AppError> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM project_directories WHERE project_id = ?1",
        params![project_id],
    )?;
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES (?1, ?2, ?3, TRUE)",
        params![id, project_id, path],
    )?;
    let dir = tx
        .query_row(
            &format!(
                "SELECT {} FROM project_directories WHERE id = ?1",
                SELECT_COLS
            ),
            params![id],
            row_to_directory,
        )
        .map_err(AppError::Database)?;
    tx.commit()?;
    Ok(dir)
}

pub fn get_directory_by_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ProjectDirectory>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {} FROM project_directories WHERE project_id = ?1 AND is_primary = TRUE",
            SELECT_COLS
        ),
        params![project_id],
        row_to_directory,
    )
    .optional()
    .map_err(AppError::Database)
}

pub fn unlink_directory(conn: &Connection, project_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM project_directories WHERE project_id = ?1",
        params![project_id],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, directory_id: &str, status: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE project_directories SET status = ?1 WHERE id = ?2",
        params![status, directory_id],
    )?;
    if affected == 0 {
        return Err(AppError::DirectoryNotFound(directory_id.to_string()));
    }
    Ok(())
}

pub fn touch_scanned(conn: &Connection, directory_id: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE project_directories SET last_scanned_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![directory_id],
    )?;
    if affected == 0 {
        return Err(AppError::DirectoryNotFound(directory_id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES (?1, 'acc1', 'Proj')",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn test_link_and_get_directory() {
        let mut conn = setup_db();
        create_project(&conn, "p1");

        let dir = link_directory(&mut conn, "p1", "/tmp/stage-a").unwrap();
        assert_eq!(dir.project_id, "p1");
        assert_eq!(dir.path, "/tmp/stage-a");
        assert!(dir.is_primary);
        assert_eq!(dir.status, "ok");

        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert_eq!(fetched.id, dir.id);
    }

    #[test]
    fn test_link_replaces_existing() {
        let mut conn = setup_db();
        create_project(&conn, "p1");

        let first = link_directory(&mut conn, "p1", "/tmp/old").unwrap();
        let second = link_directory(&mut conn, "p1", "/tmp/new").unwrap();
        assert_ne!(first.id, second.id);

        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert_eq!(fetched.path, "/tmp/new");
    }

    #[test]
    fn test_get_directory_none_when_unlinked() {
        let mut conn = setup_db();
        create_project(&conn, "p1");
        assert!(get_directory_by_project(&conn, "p1").unwrap().is_none());

        link_directory(&mut conn, "p1", "/tmp/x").unwrap();
        unlink_directory(&conn, "p1").unwrap();
        assert!(get_directory_by_project(&conn, "p1").unwrap().is_none());
    }

    #[test]
    fn test_set_status_and_touch_scanned() {
        let mut conn = setup_db();
        create_project(&conn, "p1");
        let dir = link_directory(&mut conn, "p1", "/tmp/x").unwrap();

        set_status(&conn, &dir.id, "missing").unwrap();
        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert_eq!(fetched.status, "missing");
        assert!(fetched.last_scanned_at.is_none());

        touch_scanned(&conn, &dir.id).unwrap();
        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert!(fetched.last_scanned_at.is_some());
    }

    #[test]
    fn test_set_status_not_found() {
        let conn = setup_db();
        let result = set_status(&conn, "nonexistent", "ok");
        assert!(matches!(result, Err(AppError::DirectoryNotFound(_))));
    }

    #[test]
    fn test_link_failure_does_not_lose_own_existing_link() {
        // 案件Bが自分自身の既存の紐付け(/tmp/own)を持った状態で、
        // 既に案件Aが使っている /tmp/shared に付け替えようとして UNIQUE(path) 違反で失敗した場合、
        // 案件B自身の元の紐付け(/tmp/own)が黙って消えてはならない（非トランザクションのDELETE→INSERTのバグ）。
        let mut conn = setup_db();
        create_project(&conn, "a");
        create_project(&conn, "b");

        link_directory(&mut conn, "a", "/tmp/shared").unwrap();
        let original_b = link_directory(&mut conn, "b", "/tmp/own").unwrap();

        let result = link_directory(&mut conn, "b", "/tmp/shared");
        assert!(result.is_err(), "UNIQUE(path)違反でエラーになるはず");

        let fetched_b = get_directory_by_project(&conn, "b")
            .unwrap()
            .expect("案件B自身の元の紐付けが残っていること");
        assert_eq!(fetched_b.id, original_b.id);
        assert_eq!(fetched_b.path, "/tmp/own");
    }
}
