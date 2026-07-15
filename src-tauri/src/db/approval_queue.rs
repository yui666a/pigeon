//! Sensitive 操作の承認キュー（approval_queue テーブル）。
//! 非 UI driver（MCP / Agent）の Sensitive 操作は dispatch がここへ積んで保留する。
//! 承認・却下の判断 UI と承認後の再実行は Phase 5-2（ADR 0004）。

use rusqlite::Connection;

use crate::error::AppError;

/// 承認キューの1行。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalRow {
    pub id: i64,
    pub ts: String,
    pub use_case: String,
    pub input_json: String,
    pub driver: String,
    pub status: String,
    pub decided_ts: Option<String>,
}

/// 保留エントリを積む。戻り値はキュー ID。
pub fn enqueue(
    conn: &Connection,
    ts: &str,
    use_case: &str,
    input_json: &str,
    driver: &str,
) -> Result<i64, AppError> {
    conn.execute(
        "INSERT INTO approval_queue (ts, use_case, input_json, driver)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![ts, use_case, input_json, driver],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 保留中のエントリを古い順に返す。
pub fn list_pending(conn: &Connection) -> Result<Vec<ApprovalRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, use_case, input_json, driver, status, decided_ts
         FROM approval_queue WHERE status = 'pending' ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ApprovalRow {
                id: row.get(0)?,
                ts: row.get(1)?,
                use_case: row.get(2)?,
                input_json: row.get(3)?,
                driver: row.get(4)?,
                status: row.get(5)?,
                decided_ts: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 保留エントリを承認または却下する。pending 以外への適用や不在 ID はエラー。
pub fn decide(conn: &Connection, id: i64, approve: bool, decided_ts: &str) -> Result<(), AppError> {
    let status = if approve { "approved" } else { "rejected" };
    let updated = conn.execute(
        "UPDATE approval_queue SET status = ?1, decided_ts = ?2
         WHERE id = ?3 AND status = 'pending'",
        rusqlite::params![status, decided_ts, id],
    )?;
    if updated == 0 {
        return Err(AppError::Validation(format!(
            "approval queue entry {id} not found or already decided"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    #[test]
    fn test_enqueue_and_list_pending() {
        let conn = setup_db();
        let id = enqueue(
            &conn,
            "2026-07-15T10:00:00Z",
            "send_mail",
            r#"{"account_id":"acc1"}"#,
            "mcp",
        )
        .unwrap();

        let pending = list_pending(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        assert_eq!(pending[0].use_case, "send_mail");
        assert_eq!(pending[0].status, "pending");
        assert_eq!(pending[0].driver, "mcp");
    }

    #[test]
    fn test_decide_approves_and_removes_from_pending() {
        let conn = setup_db();
        let id = enqueue(&conn, "t", "send_mail", "{}", "agent").unwrap();

        decide(&conn, id, true, "2026-07-15T11:00:00Z").unwrap();
        assert!(list_pending(&conn).unwrap().is_empty());

        // 二重判断は拒否
        let err = decide(&conn, id, false, "2026-07-15T12:00:00Z");
        assert!(matches!(err, Err(AppError::Validation(_))));
    }

    #[test]
    fn test_decide_missing_id_errors() {
        let conn = setup_db();
        assert!(matches!(
            decide(&conn, 999, true, "t"),
            Err(AppError::Validation(_))
        ));
    }
}
