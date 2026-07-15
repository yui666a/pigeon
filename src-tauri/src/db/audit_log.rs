//! 監査ログ（audit_log テーブル）の CRUD。書き込みは dispatch バスの
//! SqliteAuditSink（usecase/audit.rs）だけが行う。

use rusqlite::Connection;

use crate::error::AppError;

/// 監査ログの1行（読み出し用）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogRow {
    pub id: i64,
    pub ts: String,
    pub use_case: String,
    pub risk: String,
    pub driver: String,
    pub input_summary: String,
}

pub fn insert(
    conn: &Connection,
    ts: &str,
    use_case: &str,
    risk: &str,
    driver: &str,
    input_summary: &str,
) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO audit_log (ts, use_case, risk, driver, input_summary)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![ts, use_case, risk, driver, input_summary],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 新しい順に最大 limit 件を返す。
pub fn list_recent(conn: &Connection, limit: u32) -> Result<Vec<AuditLogRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, use_case, risk, driver, input_summary
         FROM audit_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok(AuditLogRow {
                id: row.get(0)?,
                ts: row.get(1)?,
                use_case: row.get(2)?,
                risk: row.get(3)?,
                driver: row.get(4)?,
                input_summary: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    #[test]
    fn test_insert_and_list_recent() {
        let conn = setup_db();
        insert(
            &conn,
            "2026-07-15T10:00:00Z",
            "delete_mail",
            "sensitive",
            "ui",
            "{}",
        )
        .unwrap();
        insert(
            &conn,
            "2026-07-15T10:01:00Z",
            "set_flagged",
            "reversible",
            "ui",
            "{}",
        )
        .unwrap();

        let rows = list_recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        // 新しい順
        assert_eq!(rows[0].use_case, "set_flagged");
        assert_eq!(rows[1].use_case, "delete_mail");
        assert_eq!(rows[1].risk, "sensitive");
        assert_eq!(rows[1].driver, "ui");
    }

    #[test]
    fn test_list_recent_respects_limit() {
        let conn = setup_db();
        for i in 0..5 {
            insert(
                &conn,
                "2026-07-15T10:00:00Z",
                &format!("uc{i}"),
                "reversible",
                "ui",
                "{}",
            )
            .unwrap();
        }
        assert_eq!(list_recent(&conn, 3).unwrap().len(), 3);
    }
}
