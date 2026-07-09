use crate::error::AppError;
use crate::models::directory::{ProjectFile, ProjectFileEntry};
use rusqlite::{params, Connection};
use uuid::Uuid;

/// インベントリをスナップショットとして全置換する（スペック§4: 消えたファイルはハードデリート）。
/// トランザクション内で実行するため途中失敗しても前回の状態が残り、冪等にやり直せる。
pub fn replace_inventory(
    conn: &mut Connection,
    directory_id: &str,
    entries: &[ProjectFileEntry],
) -> Result<(), AppError> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM project_files WHERE directory_id = ?1",
        params![directory_id],
    )?;
    for e in entries {
        tx.execute(
            "INSERT INTO project_files
                (id, directory_id, relative_path, size_bytes, mtime,
                 content_hash, content_kind, extract_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                directory_id,
                e.relative_path,
                e.size_bytes,
                e.mtime,
                e.content_hash,
                e.content_kind,
                e.extract_status,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn list_files(conn: &Connection, directory_id: &str) -> Result<Vec<ProjectFile>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, directory_id, relative_path, size_bytes, mtime,
                content_hash, content_kind, extract_status, indexed_at
         FROM project_files WHERE directory_id = ?1 ORDER BY relative_path",
    )?;
    let files = stmt
        .query_map(params![directory_id], |row| {
            Ok(ProjectFile {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                relative_path: row.get(2)?,
                size_bytes: row.get(3)?,
                mtime: row.get(4)?,
                content_hash: row.get(5)?,
                content_kind: row.get(6)?,
                extract_status: row.get(7)?,
                indexed_at: row.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn setup_dir(conn: &mut Connection) -> String {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(conn, "p1", "/tmp/x")
            .unwrap()
            .id
    }

    fn entry(path: &str, size: i64) -> ProjectFileEntry {
        ProjectFileEntry {
            relative_path: path.to_string(),
            size_bytes: size,
            mtime: "2026-07-09T00:00:00Z".to_string(),
            content_hash: None,
            content_kind: "other".to_string(),
            extract_status: "unsupported".to_string(),
        }
    }

    #[test]
    fn test_replace_inventory_inserts_and_lists() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);

        replace_inventory(&mut conn, &dir_id, &[entry("a.pdf", 100), entry("sub/b.txt", 20)])
            .unwrap();

        let files = list_files(&conn, &dir_id).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].relative_path, "a.pdf"); // relative_path 順
        assert_eq!(files[1].relative_path, "sub/b.txt");
    }

    #[test]
    fn test_replace_inventory_removes_deleted_files() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);

        replace_inventory(&mut conn, &dir_id, &[entry("a.pdf", 100), entry("b.txt", 20)])
            .unwrap();
        // b.txt が消えた状態で再スキャン
        replace_inventory(&mut conn, &dir_id, &[entry("a.pdf", 100)]).unwrap();

        let files = list_files(&conn, &dir_id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "a.pdf");
    }

    #[test]
    fn test_replace_inventory_empty_is_ok() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);
        replace_inventory(&mut conn, &dir_id, &[]).unwrap();
        assert!(list_files(&conn, &dir_id).unwrap().is_empty());
    }
}
