use std::sync::Mutex;

use crate::usecase::{Driver, Risk};

/// 監査ログの 1 エントリ。4-2 では use_case / risk / driver のみ。
/// timestamp と input 概要は 4-4 の SQLite スキーマ確定時に足す。
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub use_case: String,
    pub risk: Risk,
    pub driver: Driver,
}

impl AuditEntry {
    pub fn new(use_case: &str, risk: Risk, driver: Driver) -> Self {
        Self {
            use_case: use_case.to_string(),
            risk,
            driver,
        }
    }
}

/// 監査ログのシンク。dispatch が Reversible/Sensitive の実行時に record する。
/// 4-2 の実体は NoOp / InMemory のみ。SQLite シンクは 4-4。
pub trait AuditSink: Send + Sync {
    fn record(&self, entry: AuditEntry);
}

/// 記録を捨てる既定シンク（4-2 の read 系は監査対象外）。
pub struct NoOpAuditSink;

impl AuditSink for NoOpAuditSink {
    fn record(&self, _entry: AuditEntry) {}
}

/// テスト用: record を蓄積するシンク（4-4 の SQLite 実装の差し替え先）。
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
    fn record(&self, entry: AuditEntry) {
        if let Ok(mut v) = self.entries.lock() {
            v.push(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::{Driver, Risk};

    #[test]
    fn test_noop_sink_discards() {
        let sink = NoOpAuditSink;
        // panic せず捨てるだけ
        sink.record(AuditEntry::new("x", Risk::Reversible, Driver::Ui));
    }

    #[test]
    fn test_in_memory_sink_accumulates() {
        let sink = InMemoryAuditSink::new();
        sink.record(AuditEntry::new("send_mail", Risk::Sensitive, Driver::Agent));
        sink.record(AuditEntry::new("move_mail", Risk::Reversible, Driver::Ui));

        let entries = sink.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].use_case, "send_mail");
        assert_eq!(entries[0].risk, Risk::Sensitive);
        assert_eq!(entries[0].driver, Driver::Agent);
        assert_eq!(entries[1].use_case, "move_mail");
    }
}
