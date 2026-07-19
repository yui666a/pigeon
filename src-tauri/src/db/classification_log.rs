//! AI 分類の全判断を記録するログ。
//!
//! `mail_project_assignments` は「assign を選び、かつ確信度が閾値以上」の
//! ケースしか残さないため、AI が出した確信度の生の分布が観測できない。
//! 破棄された低確信 assign や create / unclassified も含めて記録し、
//! キャリブレーションを事後に検証できるようにする
//! （設計: docs/design/2026-07-20-classification-observability-design.md）。

use rusqlite::{params, Connection};

use crate::error::AppError;

/// 記録する1件の判断。本文由来のテキスト（reason・件名）は持たせない。
#[derive(Debug, Clone)]
pub struct ClassificationLogEntry<'a> {
    pub mail_id: &'a str,
    pub account_id: &'a str,
    /// 'assign' | 'create' | 'unclassified'
    pub action: &'a str,
    pub project_id: Option<&'a str>,
    /// 提案時点の案件パス。案件が後で消えても意味を保つためのスナップショット
    pub project_path: Option<String>,
    pub proposed_name: Option<&'a str>,
    pub confidence: f64,
    /// 確信度ゲートを通過して実際に割り当てられたか
    pub persisted: bool,
    /// "provider:model" 形式。モデルを変えると確信度の性質も変わるため軸として残す
    pub model: Option<&'a str>,
}

/// 判断を1件記録する。
///
/// 記録はあくまで観測用であり、分類そのものの成否を左右しない。ただし
/// 呼び出し側が分類本体と同じトランザクションに含めることで、割り当てと
/// ログが食い違う状態を作らない。
pub fn insert_log(conn: &Connection, entry: &ClassificationLogEntry<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO classification_log
            (mail_id, account_id, action, project_id, project_path,
             proposed_name, confidence, persisted, model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.mail_id,
            entry.account_id,
            entry.action,
            entry.project_id,
            entry.project_path,
            entry.proposed_name,
            entry.confidence,
            entry.persisted as i64,
            entry.model,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{insert_test_mail, setup_db};

    fn entry<'a>(action: &'a str, confidence: f64, persisted: bool) -> ClassificationLogEntry<'a> {
        ClassificationLogEntry {
            mail_id: "m1",
            account_id: "acc1",
            action,
            project_id: None,
            project_path: None,
            proposed_name: None,
            confidence,
            persisted,
            model: Some("gemini_vertex:gemini-3.5-flash"),
        }
    }

    #[test]
    fn test_insert_log_records_discarded_judgement() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Subject");

        // 確信度ゲートで破棄された判断こそ、観測できないと困る
        insert_log(&conn, &entry("unclassified", 0.2, false)).unwrap();

        let (action, confidence, persisted, model): (String, f64, i64, String) = conn
            .query_row(
                "SELECT action, confidence, persisted, model FROM classification_log",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(action, "unclassified");
        assert!((confidence - 0.2).abs() < f64::EPSILON);
        assert_eq!(persisted, 0);
        assert_eq!(model, "gemini_vertex:gemini-3.5-flash");
    }

    #[test]
    fn test_insert_log_keeps_project_path_snapshot() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Subject");

        let e = ClassificationLogEntry {
            project_path: Some("親 > 子".into()),
            ..entry("assign", 0.95, true)
        };
        insert_log(&conn, &e).unwrap();

        let path: String = conn
            .query_row("SELECT project_path FROM classification_log", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(path, "親 > 子");
    }

    #[test]
    fn test_insert_log_accumulates_per_mail() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Subject");

        // 同じメールを再分類しても上書きせず積み上がる（履歴として追える）
        insert_log(&conn, &entry("unclassified", 0.2, false)).unwrap();
        insert_log(&conn, &entry("assign", 0.95, true)).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM classification_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
