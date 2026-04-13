use rusqlite::Connection;
use crate::error::AppError;

pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS accounts (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            email       TEXT NOT NULL,
            imap_host   TEXT NOT NULL,
            imap_port   INTEGER NOT NULL DEFAULT 993,
            smtp_host   TEXT NOT NULL,
            smtp_port   INTEGER NOT NULL DEFAULT 587,
            auth_type   TEXT NOT NULL CHECK(auth_type IN ('plain', 'oauth2')),
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS mails (
            id              TEXT PRIMARY KEY,
            account_id      TEXT NOT NULL REFERENCES accounts(id),
            folder          TEXT NOT NULL,
            message_id      TEXT NOT NULL,
            in_reply_to     TEXT,
            'references'    TEXT,
            from_addr       TEXT NOT NULL,
            to_addr         TEXT NOT NULL,
            cc_addr         TEXT,
            subject         TEXT NOT NULL,
            body_text       TEXT,
            body_html       TEXT,
            date            DATETIME NOT NULL,
            has_attachments BOOLEAN DEFAULT FALSE,
            raw_size        INTEGER,
            uid             INTEGER NOT NULL,
            flags           TEXT,
            fetched_at      DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_migrations_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"accounts".to_string()));
        assert!(tables.contains(&"mails".to_string()));
        assert!(tables.contains(&"settings".to_string()));
    }

    #[test]
    fn test_run_migrations_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }
}
