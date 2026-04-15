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

fn migrate_v3(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id          TEXT PRIMARY KEY,
            account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            description TEXT,
            color       TEXT,
            is_archived BOOLEAN DEFAULT FALSE,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_projects_account ON projects(account_id);

        CREATE TABLE IF NOT EXISTS mail_project_assignments (
            mail_id        TEXT PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE,
            project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            assigned_by    TEXT NOT NULL CHECK(assigned_by IN ('ai', 'user')),
            confidence     REAL,
            corrected_from TEXT,
            created_at     DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_mpa_project ON mail_project_assignments(project_id);

        CREATE TRIGGER IF NOT EXISTS trg_mpa_account_check
        BEFORE INSERT ON mail_project_assignments
        BEGIN
            SELECT CASE
                WHEN (SELECT account_id FROM mails WHERE id = NEW.mail_id)
                  != (SELECT account_id FROM projects WHERE id = NEW.project_id)
                THEN RAISE(ABORT, 'mail and project must belong to the same account')
            END;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_mpa_account_check_update
        BEFORE UPDATE OF project_id ON mail_project_assignments
        BEGIN
            SELECT CASE
                WHEN (SELECT account_id FROM mails WHERE id = NEW.mail_id)
                  != (SELECT account_id FROM projects WHERE id = NEW.project_id)
                THEN RAISE(ABORT, 'mail and project must belong to the same account')
            END;
        END;

        CREATE TABLE IF NOT EXISTS correction_log (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            mail_id        TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
            from_project   TEXT REFERENCES projects(id) ON DELETE SET NULL,
            to_project     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            corrected_at   DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        ",
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

    if version < 3 {
        migrate_v3(conn)?;
        version = 3;
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
    fn test_foreign_keys_enabled() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Verify foreign keys are ON: insert a mail referencing a non-existent account should fail
        let result = conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'nonexistent', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Test', '2026-01-01', 1)",
            [],
        );
        assert!(
            result.is_err(),
            "foreign key constraint should have prevented insert"
        );
    }

    #[test]
    fn test_v3_migration_creates_projects_and_assignments() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"mail_project_assignments".to_string()));
        assert!(tables.contains(&"correction_log".to_string()));

        // Verify schema version is 3
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn test_v3_migration_account_trigger_prevents_cross_account() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Insert two accounts
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Account 1', 'a1@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'Account 2', 'a2@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        // Insert a mail belonging to acc1
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('mail1', 'acc1', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Subject', '2026-01-01', 1)",
            [],
        ).unwrap();

        // Insert a project belonging to acc2
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('proj2', 'acc2', 'Project 2')",
            [],
        )
        .unwrap();

        // Attempting to assign mail (acc1) to project (acc2) should fail
        let result = conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by)
             VALUES ('mail1', 'proj2', 'ai')",
            [],
        );
        assert!(
            result.is_err(),
            "cross-account assignment should be rejected by trigger"
        );
    }

    #[test]
    fn test_v3_migration_same_account_assignment_succeeds() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Insert account
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Account 1', 'a1@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        // Insert mail and project both belonging to acc1
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('mail1', 'acc1', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Subject', '2026-01-01', 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('proj1', 'acc1', 'Project 1')",
            [],
        )
        .unwrap();

        // Same-account assignment should succeed
        let result = conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by, confidence)
             VALUES ('mail1', 'proj1', 'ai', 0.95)",
            [],
        );
        assert!(result.is_ok(), "same-account assignment should succeed");
    }

    #[test]
    fn test_v3_cascade_delete_project() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Insert account, mail, and project
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Account 1', 'a1@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('mail1', 'acc1', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Subject', '2026-01-01', 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('proj1', 'acc1', 'Project 1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by)
             VALUES ('mail1', 'proj1', 'user')",
            [],
        )
        .unwrap();

        // Verify assignment exists
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 'mail1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Delete the project — assignment should cascade-delete
        conn.execute("DELETE FROM projects WHERE id = 'proj1'", [])
            .unwrap();

        let count_after: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 'mail1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count_after, 0,
            "assignment should be cascade-deleted when project is deleted"
        );
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

        // Run migrations — should detect V1, apply V2 and V3
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

        // Schema version should be 3 (latest)
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }
}
