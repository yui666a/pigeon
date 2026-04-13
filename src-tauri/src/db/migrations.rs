use crate::error::AppError;
use rusqlite::{params, Connection};

fn get_schema_version(conn: &Connection) -> Result<i32, AppError> {
    // Create schema_version table if not exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );",
    )?;

    let count: i32 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))?;

    if count == 0 {
        // Check if accounts table already exists (pre-versioning DB)
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='accounts'",
            [],
            |row| row.get(0),
        )?;
        let initial_version = if table_exists { 1 } else { 0 };
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![initial_version],
        )?;
        Ok(initial_version)
    } else {
        let version: i32 =
            conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;
        Ok(version)
    }
}

fn set_schema_version(conn: &Connection, version: i32) -> Result<(), AppError> {
    conn.execute("UPDATE schema_version SET version = ?1", params![version])?;
    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<(), AppError> {
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

fn migrate_v2(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "ALTER TABLE accounts ADD COLUMN provider TEXT NOT NULL DEFAULT 'other'
            CHECK(provider IN ('google', 'other'));",
    )?;
    Ok(())
}

pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    let mut version = get_schema_version(conn)?;

    if version < 1 {
        migrate_v1(conn)?;
        version = 1;
        set_schema_version(conn, version)?;
    }

    if version < 2 {
        migrate_v2(conn)?;
        version = 2;
        set_schema_version(conn, version)?;
    }

    let _ = version;

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
        assert!(tables.contains(&"schema_version".to_string()));
    }

    #[test]
    fn test_run_migrations_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn test_v2_migration_adds_provider_column() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify provider column exists with correct default
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('test1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        let provider: String = conn
            .query_row(
                "SELECT provider FROM accounts WHERE id = 'test1'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(provider, "other");
    }

    #[test]
    fn test_v2_migration_provider_check_constraint() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Valid provider 'google'
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('g1', 'Gmail', 'user@gmail.com', 'imap.gmail.com', 'smtp.gmail.com', 'oauth2', 'google')",
            [],
        ).unwrap();

        // Invalid provider should fail
        let result = conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('x1', 'Bad', 'user@bad.com', 'imap.bad.com', 'smtp.bad.com', 'plain', 'yahoo')",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_v2_migration_on_existing_v1_database() {
        let conn = Connection::open_in_memory().unwrap();

        // Simulate a V1 database (tables created without provider column)
        conn.execute_batch(
            "
            CREATE TABLE accounts (
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
            CREATE TABLE mails (
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
            CREATE TABLE settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )
        .unwrap();

        // Insert a V1 account
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('old1', 'Old Account', 'old@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        // Run migrations — should detect V1, apply V2
        run_migrations(&conn).unwrap();

        // Existing account should have provider = 'other'
        let provider: String = conn
            .query_row(
                "SELECT provider FROM accounts WHERE id = 'old1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provider, "other");

        // Schema version should be 2
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }
}
