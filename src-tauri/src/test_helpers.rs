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
pub fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
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
        uid: 1,
        flags: None,
        fetched_at: "2026-04-13T00:00:00".into(),
    }
}

/// Insert a test mail with minimal parameters (for classify tests).
///
/// Creates a mail with the given `id` and `subject`, using defaults for
/// everything else, and inserts it into the database.
pub fn insert_test_mail(conn: &Connection, id: &str, subject: &str) {
    let mail = make_mail(id, &format!("<{}@test.com>", id), subject, "2026-04-13T10:00:00");
    crate::db::mails::insert_mail(conn, &mail).unwrap();
}
