use crate::error::AppError;
use crate::models::directory::CloudRule;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// project_cloud_rules テーブルの1行を CloudRule へ変換する共通マッパー。
/// カラム順は `SELECT id, directory_id, scope, relative_path, allow` に一致させること。
fn row_to_cloud_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<CloudRule> {
    Ok(CloudRule {
        id: row.get(0)?,
        directory_id: row.get(1)?,
        scope: row.get(2)?,
        relative_path: row.get(3)?,
        allow: row.get(4)?,
    })
}

pub fn set_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
    allow: bool,
) -> Result<(), AppError> {
    // directory スコープの relative_path が末尾 '/' 付きで保存されると
    // is_cloud_allowed の prefix マッチ（"{path}/"）が二重スラッシュとなり
    // ルールが機能しなくなるため、保存前に末尾の '/' を除去する。
    // 空文字になった場合はそのまま（全体ルールを表す）。
    let normalized_path = relative_path.trim_end_matches('/');
    conn.execute(
        "INSERT INTO project_cloud_rules (id, directory_id, scope, relative_path, allow)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(directory_id, scope, relative_path) DO UPDATE SET allow = ?5",
        params![Uuid::new_v4().to_string(), directory_id, scope, normalized_path, allow],
    )?;
    Ok(())
}

pub fn delete_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
) -> Result<(), AppError> {
    // set_rule と同じ正規化を適用し、末尾スラッシュ付きで呼ばれた場合でも
    // 正規化済みで保存されたルールを削除できるようにする（非対称の解消）。
    let normalized_path = relative_path.trim_end_matches('/');
    conn.execute(
        "DELETE FROM project_cloud_rules
         WHERE directory_id = ?1 AND scope = ?2 AND relative_path = ?3",
        params![directory_id, scope, normalized_path],
    )?;
    Ok(())
}

pub fn list_rules(conn: &Connection, directory_id: &str) -> Result<Vec<CloudRule>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, directory_id, scope, relative_path, allow
         FROM project_cloud_rules WHERE directory_id = ?1 ORDER BY relative_path",
    )?;
    let rules = stmt
        .query_map(params![directory_id], row_to_cloud_rule)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rules)
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
        crate::db::directories::link_directory(conn, "p1", "/tmp/x").unwrap().id
    }

    #[test]
    fn test_set_rule_upserts() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);

        set_rule(&conn, &dir_id, "directory", "図面", true).unwrap();
        set_rule(&conn, &dir_id, "directory", "図面", false).unwrap(); // 同キーは上書き

        let rules = list_rules(&conn, &dir_id).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].allow);
    }

    #[test]
    fn test_delete_rule() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);
        set_rule(&conn, &dir_id, "file", "a.txt", true).unwrap();
        delete_rule(&conn, &dir_id, "file", "a.txt").unwrap();
        assert!(list_rules(&conn, &dir_id).unwrap().is_empty());
    }

    #[test]
    fn test_set_rule_normalizes_trailing_slash() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);

        set_rule(&conn, &dir_id, "directory", "図面/", true).unwrap();

        let rules = list_rules(&conn, &dir_id).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].relative_path, "図面");
    }

    #[test]
    fn test_delete_rule_normalizes_trailing_slash() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&mut conn);

        set_rule(&conn, &dir_id, "directory", "図面/", true).unwrap();
        delete_rule(&conn, &dir_id, "directory", "図面/").unwrap(); // 末尾スラッシュ付きでも削除できる

        assert!(list_rules(&conn, &dir_id).unwrap().is_empty());
    }
}
