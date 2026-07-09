use crate::error::AppError;
use crate::models::directory::CloudRule;
use rusqlite::{params, Connection};
use uuid::Uuid;

pub fn set_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
    allow: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_cloud_rules (id, directory_id, scope, relative_path, allow)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(directory_id, scope, relative_path) DO UPDATE SET allow = ?5",
        params![Uuid::new_v4().to_string(), directory_id, scope, relative_path, allow],
    )?;
    Ok(())
}

pub fn delete_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM project_cloud_rules
         WHERE directory_id = ?1 AND scope = ?2 AND relative_path = ?3",
        params![directory_id, scope, relative_path],
    )?;
    Ok(())
}

pub fn list_rules(conn: &Connection, directory_id: &str) -> Result<Vec<CloudRule>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, directory_id, scope, relative_path, allow
         FROM project_cloud_rules WHERE directory_id = ?1 ORDER BY relative_path",
    )?;
    let rules = stmt
        .query_map(params![directory_id], |row| {
            Ok(CloudRule {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                scope: row.get(2)?,
                relative_path: row.get(3)?,
                allow: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn setup_dir(conn: &Connection) -> String {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(conn, "p1", "/tmp/x").unwrap().id
    }

    #[test]
    fn test_set_rule_upserts() {
        let conn = setup_db();
        let dir_id = setup_dir(&conn);

        set_rule(&conn, &dir_id, "directory", "図面", true).unwrap();
        set_rule(&conn, &dir_id, "directory", "図面", false).unwrap(); // 同キーは上書き

        let rules = list_rules(&conn, &dir_id).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].allow);
    }

    #[test]
    fn test_delete_rule() {
        let conn = setup_db();
        let dir_id = setup_dir(&conn);
        set_rule(&conn, &dir_id, "file", "a.txt", true).unwrap();
        delete_rule(&conn, &dir_id, "file", "a.txt").unwrap();
        assert!(list_rules(&conn, &dir_id).unwrap().is_empty());
    }
}
