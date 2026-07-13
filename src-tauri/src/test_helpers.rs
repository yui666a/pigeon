//! Shared test helper functions used across multiple test modules.
//!
//! This module consolidates duplicated setup_db / make_mail / insert_test_mail
//! helpers that were previously copy-pasted in commands and db test modules.

use crate::db::migrations;
use crate::models::mail::Mail;
use rusqlite::Connection;

/// Create an in-memory SQLite database with migrations applied and a default test account.
///
/// The test account has id="acc1", email="test@example.com".
///
/// 本番 (lib.rs) と同様に外部キー強制を明示的に有効化する。
/// bundled SQLite は SQLITE_DEFAULT_FOREIGN_KEYS=1 でビルドされるためデフォルトでも
/// ON になるが、ビルド設定に依存せず本番と同じ挙動を保証するため明示する。
pub fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    migrations::run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
        [],
    )
    .unwrap();
    conn
}

/// Create a test Mail with the given parameters.
///
/// Uses account_id="acc1" and sensible defaults for other fields.
pub fn make_mail(id: &str, message_id: &str, subject: &str, date: &str) -> Mail {
    Mail {
        id: id.into(),
        account_id: "acc1".into(),
        folder: "INBOX".into(),
        message_id: message_id.into(),
        in_reply_to: None,
        references: None,
        from_addr: "sender@example.com".into(),
        to_addr: "me@example.com".into(),
        cc_addr: None,
        subject: subject.into(),
        body_text: Some("Hello".into()),
        body_html: None,
        date: date.into(),
        has_attachments: false,
        raw_size: None,
        // (account_id, folder, uid) は UNIQUE (migrate_v6) のため、
        // id から決定的に導出して同一アカウント内での衝突を避ける
        uid: id.bytes().fold(0u32, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u32::from(b))
        }),
        flags: None,
        is_read: false,
        is_flagged: false,
        fetched_at: "2026-04-13T00:00:00".into(),
        uid_confirmed: true,
    }
}

/// Insert a test mail with minimal parameters (for classify tests).
///
/// Creates a mail with the given `id` and `subject`, using defaults for
/// everything else, and inserts it into the database.
pub fn insert_test_mail(conn: &Connection, id: &str, subject: &str) {
    let mail = make_mail(
        id,
        &format!("<{}@test.com>", id),
        subject,
        "2026-04-13T10:00:00",
    );
    crate::db::mails::insert_mail(conn, &mail).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本番 (lib.rs) は接続直後に PRAGMA foreign_keys = ON を設定している。
    /// テスト用 DB も同じ設定でなければ FK/CASCADE 依存のバグを検出できないため、
    /// setup_db が返す接続で外部キー強制が有効であることを保証する。
    #[test]
    fn test_setup_db_enables_foreign_keys() {
        let conn = setup_db();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1, "setup_db must enable PRAGMA foreign_keys");
    }

    /// FK 強制が実際に効いていること（存在しない親への子行挿入が失敗すること）を確認する。
    #[test]
    fn test_setup_db_enforces_foreign_keys() {
        let conn = setup_db();
        let mut mail = make_mail("m1", "<m1@test.com>", "Subject", "2026-04-13T10:00:00");
        mail.account_id = "no-such-account".into();
        let result = crate::db::mails::insert_mail(&conn, &mail);
        assert!(
            result.is_err(),
            "inserting a mail for a nonexistent account must violate FK"
        );
    }
}
