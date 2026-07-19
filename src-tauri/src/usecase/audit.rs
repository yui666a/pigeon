use std::sync::Mutex;

use rusqlite::Connection;
use serde_json::Value;

use crate::usecase::{Driver, Risk};

/// 監査ログの 1 エントリ。dispatch が Reversible/Sensitive の実行前に記録する。
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub ts: String,
    pub use_case: String,
    pub risk: Risk,
    pub driver: Driver,
    pub input_summary: String,
}

impl AuditEntry {
    pub fn new(use_case: &str, risk: Risk, driver: Driver, input: &Value) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339(),
            use_case: use_case.to_string(),
            risk,
            driver,
            input_summary: summarize_input(input),
        }
    }
}

/// 監査ログ用の入力要約。対象の特定（mail_id 等）には十分で、
/// 本文のような長い値は切り詰める（DB への重複保存を避ける）。
pub fn summarize_input(input: &Value) -> String {
    const MAX_STR: usize = 64;
    const MAX_TOTAL: usize = 1000;

    fn truncate_values(v: &Value) -> Value {
        match v {
            Value::String(s) if s.chars().count() > MAX_STR => {
                let head: String = s.chars().take(MAX_STR).collect();
                Value::String(format!("{head}…"))
            }
            Value::Array(items) => Value::Array(items.iter().map(truncate_values).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), truncate_values(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    let mut out = truncate_values(input).to_string();
    if out.chars().count() > MAX_TOTAL {
        out = out.chars().take(MAX_TOTAL).collect::<String>() + "…";
    }
    out
}

/// 監査ログのシンク。dispatch が Reversible/Sensitive の実行前に record する。
/// 記録失敗で操作自体は止めない（fail-open。実装側で警告ログを残す）。
pub trait AuditSink: Send + Sync {
    fn record(&self, conn: &Connection, entry: &AuditEntry);
}

/// SQLite（audit_log テーブル）への永続シンク。本番 Ctx の既定。
pub struct SqliteAuditSink;

impl AuditSink for SqliteAuditSink {
    fn record(&self, conn: &Connection, entry: &AuditEntry) {
        if let Err(e) = crate::db::audit_log::insert(
            conn,
            &entry.ts,
            &entry.use_case,
            entry.risk.as_str(),
            entry.driver.as_str(),
            &entry.input_summary,
        ) {
            eprintln!(
                "[warn] audit: failed to record {} ({:?}/{:?}): {}",
                entry.use_case, entry.risk, entry.driver, e
            );
        }
    }
}

/// 記録を捨てるシンク（監査を切りたいテスト用）。
pub struct NoOpAuditSink;

impl AuditSink for NoOpAuditSink {
    fn record(&self, _conn: &Connection, _entry: &AuditEntry) {}
}

/// テスト用: record を蓄積するシンク。
pub struct InMemoryAuditSink {
    entries: Mutex<Vec<AuditEntry>>,
}

impl InMemoryAuditSink {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// 蓄積されたエントリのスナップショット。ロック毒化時は空を返す（安全側）。
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl Default for InMemoryAuditSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink for InMemoryAuditSink {
    fn record(&self, _conn: &Connection, entry: &AuditEntry) {
        if let Ok(mut v) = self.entries.lock() {
            v.push(entry.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::test_helpers::setup_db;
    use crate::usecase::{Driver, Risk};

    #[test]
    fn test_summarize_truncates_long_strings() {
        let body = "x".repeat(500);
        let summary = summarize_input(&json!({ "mail_id": "m1", "body": body }));
        assert!(summary.contains("m1"), "対象の特定に必要な短い値は残る");
        assert!(!summary.contains(&body), "長い値は切り詰める");
        assert!(summary.len() < 300);
    }

    #[test]
    fn test_sqlite_sink_persists_entry() {
        let conn = setup_db();
        let entry = AuditEntry::new(
            "delete_mail",
            Risk::Sensitive,
            Driver::Ui,
            &json!({ "mail_id": "m1" }),
        );
        SqliteAuditSink.record(&conn, &entry);

        let rows = crate::db::audit_log::list_recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].use_case, "delete_mail");
        assert_eq!(rows[0].risk, "sensitive");
        assert_eq!(rows[0].driver, "ui");
        assert!(rows[0].input_summary.contains("m1"));
    }

    #[test]
    fn test_in_memory_sink_accumulates() {
        let conn = setup_db();
        let sink = InMemoryAuditSink::new();
        sink.record(
            &conn,
            &AuditEntry::new("send_mail", Risk::Sensitive, Driver::Agent, &json!({})),
        );
        sink.record(
            &conn,
            &AuditEntry::new("bulk_move_mails", Risk::Reversible, Driver::Ui, &json!({})),
        );

        let entries = sink.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].use_case, "send_mail");
        assert_eq!(entries[0].risk, Risk::Sensitive);
        assert_eq!(entries[0].driver, Driver::Agent);
        assert_eq!(entries[1].use_case, "bulk_move_mails");
    }

    #[test]
    fn test_noop_sink_discards() {
        let conn = setup_db();
        NoOpAuditSink.record(
            &conn,
            &AuditEntry::new("x", Risk::Reversible, Driver::Ui, &json!({})),
        );
    }
}
