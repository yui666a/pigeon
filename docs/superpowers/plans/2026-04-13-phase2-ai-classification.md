# Phase 2: AI分類 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** メールをAIで案件に自動分類する機能を追加する（手動トリガー、Ollama連携）

**Architecture:** `LlmClassifier` trait で LLM プロバイダを抽象化し、Phase 2 では `OllamaClassifier` を実装する。Rust バックエンドで分類ロジック・DB 操作を行い、React フロントエンドに案件ツリー・分類 UI を追加する。

**Tech Stack:** Rust (async-trait, reqwest, rusqlite), React 19, TypeScript, Zustand 5, Tailwind CSS v4, Vitest

**Spec:** `docs/superpowers/specs/2026-04-13-phase2-ai-classification-design.md`

---

## Task 1: PRAGMA foreign_keys 有効化 + V3 マイグレーション

**Files:**
- Modify: `src-tauri/src/lib.rs:27` (Connection::open 直後に PRAGMA 追加)
- Modify: `src-tauri/src/db/migrations.rs` (migrate_v3 追加、run_migrations 更新)

- [ ] **Step 1: `lib.rs` に PRAGMA foreign_keys = ON を追加するテストを書く**

`src-tauri/src/db/migrations.rs` のテストモジュールに追加:

```rust
#[test]
fn test_foreign_keys_enabled() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();
    let fk_enabled: i32 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fk_enabled, 1);
}
```

- [ ] **Step 2: テストが通ることを確認**

Run: `cd src-tauri && cargo test test_foreign_keys_enabled -- --nocapture`
Expected: PASS

- [ ] **Step 3: `lib.rs` に PRAGMA foreign_keys = ON を追加**

`src-tauri/src/lib.rs` の `Connection::open(&db_path)` 直後に追加:

```rust
let conn = Connection::open(&db_path).expect("Failed to open database");
conn.execute_batch("PRAGMA foreign_keys = ON;")
    .expect("Failed to enable foreign keys");
migrations::run_migrations(&conn).expect("Failed to run migrations");
```

- [ ] **Step 4: V3 マイグレーションのテストを書く**

`src-tauri/src/db/migrations.rs` のテストモジュールに追加:

```rust
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
}

#[test]
fn test_v3_migration_account_trigger_prevents_cross_account() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    // Create two accounts
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc_a', 'A', 'a@ex.com', 'imap.ex.com', 'smtp.ex.com', 'plain', 'other')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc_b', 'B', 'b@ex.com', 'imap.ex.com', 'smtp.ex.com', 'plain', 'other')",
        [],
    ).unwrap();

    // Create project for account A
    conn.execute(
        "INSERT INTO projects (id, account_id, name) VALUES ('proj_a', 'acc_a', 'Project A')",
        [],
    ).unwrap();

    // Create mail for account B
    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
         VALUES ('mail_b', 'acc_b', 'INBOX', '<m@ex>', 'x@ex.com', 'b@ex.com', 'Test', '2026-04-13', 1)",
        [],
    ).unwrap();

    // Cross-account assignment should fail
    let result = conn.execute(
        "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by, confidence)
         VALUES ('mail_b', 'proj_a', 'ai', 0.8)",
        [],
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("mail and project must belong to the same account"));
}

#[test]
fn test_v3_migration_same_account_assignment_succeeds() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc1', 'Test', 'a@ex.com', 'imap.ex.com', 'smtp.ex.com', 'plain', 'other')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO projects (id, account_id, name) VALUES ('proj1', 'acc1', 'Project')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
         VALUES ('mail1', 'acc1', 'INBOX', '<m@ex>', 'x@ex.com', 'a@ex.com', 'Test', '2026-04-13', 1)",
        [],
    ).unwrap();

    let result = conn.execute(
        "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by, confidence)
         VALUES ('mail1', 'proj1', 'ai', 0.8)",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_v3_cascade_delete_project() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc1', 'Test', 'a@ex.com', 'imap.ex.com', 'smtp.ex.com', 'plain', 'other')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO projects (id, account_id, name) VALUES ('proj1', 'acc1', 'Project')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
         VALUES ('mail1', 'acc1', 'INBOX', '<m@ex>', 'x@ex.com', 'a@ex.com', 'Test', '2026-04-13', 1)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by, confidence)
         VALUES ('mail1', 'proj1', 'ai', 0.8)",
        [],
    ).unwrap();

    // Delete project should cascade to assignments
    conn.execute("DELETE FROM projects WHERE id = 'proj1'", []).unwrap();
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM mail_project_assignments WHERE project_id = 'proj1'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 5: テストを実行して失敗することを確認**

Run: `cd src-tauri && cargo test test_v3 -- --nocapture`
Expected: FAIL (projects table does not exist)

- [ ] **Step 6: migrate_v3 を実装**

`src-tauri/src/db/migrations.rs` に追加:

```rust
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
```

`run_migrations` に追加:

```rust
if version < 3 {
    migrate_v3(conn)?;
    version = 3;
    set_schema_version(conn, version)?;
}
```

- [ ] **Step 7: テストを実行して全件 PASS を確認**

Run: `cd src-tauri && cargo test -- --nocapture`
Expected: ALL PASS

- [ ] **Step 8: コミット**

```bash
git add src-tauri/src/db/migrations.rs src-tauri/src/lib.rs
git commit -m "feat(db): V3マイグレーション追加（projects, assignments, correction_log, FK有効化）"
```

---

## Task 2: Project モデル + DB CRUD

**Files:**
- Create: `src-tauri/src/models/project.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Create: `src-tauri/src/db/projects.rs`
- Modify: `src-tauri/src/db/mod.rs`
- Modify: `src-tauri/src/error.rs`

- [ ] **Step 1: Project モデルを作成**

`src-tauri/src/models/project.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub is_archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
}
```

- [ ] **Step 2: models/mod.rs にモジュール登録**

```rust
pub mod account;
pub mod mail;
pub mod project;
```

- [ ] **Step 3: AppError に ProjectNotFound を追加**

`src-tauri/src/error.rs` に追加:

```rust
#[error("Project not found: {0}")]
ProjectNotFound(String),
```

- [ ] **Step 4: projects CRUD のテストを書く**

`src-tauri/src/db/projects.rs`:

```rust
use crate::error::AppError;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};
use rusqlite::{params, Connection};
use uuid::Uuid;

pub fn insert_project(conn: &Connection, req: &CreateProjectRequest) -> Result<Project, AppError> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, account_id, name, description, color) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, req.account_id, req.name, req.description, req.color],
    )?;
    get_project(conn, &id)
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Project, AppError> {
    conn.query_row(
        "SELECT id, account_id, name, description, color, is_archived, created_at, updated_at
         FROM projects WHERE id = ?1",
        params![id],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                account_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                color: row.get(4)?,
                is_archived: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .map_err(|_| AppError::ProjectNotFound(id.to_string()))
}

pub fn list_projects(conn: &Connection, account_id: &str) -> Result<Vec<Project>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, description, color, is_archived, created_at, updated_at
         FROM projects WHERE account_id = ?1 AND is_archived = FALSE ORDER BY created_at",
    )?;
    let projects = stmt
        .query_map(params![account_id], |row| {
            Ok(Project {
                id: row.get(0)?,
                account_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                color: row.get(4)?,
                is_archived: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(projects)
}

pub fn update_project(
    conn: &Connection,
    id: &str,
    req: &UpdateProjectRequest,
) -> Result<Project, AppError> {
    let current = get_project(conn, id)?;
    let name = req.name.as_deref().unwrap_or(&current.name);
    let description = req.description.as_ref().or(current.description.as_ref());
    let color = req.color.as_ref().or(current.color.as_ref());
    conn.execute(
        "UPDATE projects SET name = ?1, description = ?2, color = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?4",
        params![name, description, color, id],
    )?;
    get_project(conn, id)
}

pub fn archive_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE projects SET is_archived = TRUE, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    if affected == 0 {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(())
}

pub fn delete_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let affected = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest};
    use crate::db::accounts;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();
        accounts::insert_account_with_id(
            &conn,
            "acc1",
            &CreateAccountRequest {
                name: "Test".into(),
                email: "test@example.com".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 587,
                auth_type: AuthType::Plain,
                provider: AccountProvider::Other,
                password: None,
            },
        ).unwrap();
        conn
    }

    #[test]
    fn test_insert_and_get_project() {
        let conn = setup_db();
        let project = insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "案件A".into(),
            description: Some("テスト案件".into()),
            color: Some("#ff0000".into()),
        }).unwrap();
        assert_eq!(project.name, "案件A");
        assert_eq!(project.account_id, "acc1");
        let fetched = get_project(&conn, &project.id).unwrap();
        assert_eq!(fetched.name, "案件A");
    }

    #[test]
    fn test_list_projects_excludes_archived() {
        let conn = setup_db();
        let p1 = insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Active".into(),
            description: None,
            color: None,
        }).unwrap();
        let p2 = insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Archived".into(),
            description: None,
            color: None,
        }).unwrap();
        archive_project(&conn, &p2.id).unwrap();
        let projects = list_projects(&conn, "acc1").unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, p1.id);
    }

    #[test]
    fn test_update_project() {
        let conn = setup_db();
        let project = insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Old Name".into(),
            description: None,
            color: None,
        }).unwrap();
        let updated = update_project(&conn, &project.id, &UpdateProjectRequest {
            name: Some("New Name".into()),
            description: None,
            color: Some("#00ff00".into()),
        }).unwrap();
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.color, Some("#00ff00".to_string()));
    }

    #[test]
    fn test_delete_project() {
        let conn = setup_db();
        let project = insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "To Delete".into(),
            description: None,
            color: None,
        }).unwrap();
        delete_project(&conn, &project.id).unwrap();
        assert!(get_project(&conn, &project.id).is_err());
    }

    #[test]
    fn test_get_nonexistent_project() {
        let conn = setup_db();
        assert!(get_project(&conn, "nonexistent").is_err());
    }
}
```

- [ ] **Step 5: db/mod.rs にモジュール登録**

```rust
pub mod accounts;
pub mod mails;
pub mod migrations;
pub mod projects;
```

- [ ] **Step 6: テストを実行して PASS を確認**

Run: `cd src-tauri && cargo test db::projects -- --nocapture`
Expected: ALL PASS

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/models/project.rs src-tauri/src/models/mod.rs \
        src-tauri/src/db/projects.rs src-tauri/src/db/mod.rs \
        src-tauri/src/error.rs
git commit -m "feat(db): Project モデルと CRUD 操作を実装"
```

---

## Task 3: Assignments DB CRUD

**Files:**
- Create: `src-tauri/src/db/assignments.rs`
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: assignments CRUD を実装（テスト付き）**

`src-tauri/src/db/assignments.rs`:

```rust
use crate::error::AppError;
use crate::models::mail::Mail;
use rusqlite::{params, Connection};

pub fn assign_mail(
    conn: &Connection,
    mail_id: &str,
    project_id: &str,
    assigned_by: &str,
    confidence: Option<f64>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO mail_project_assignments (mail_id, project_id, assigned_by, confidence)
         VALUES (?1, ?2, ?3, ?4)",
        params![mail_id, project_id, assigned_by, confidence],
    )?;
    Ok(())
}

pub fn approve_classification(
    conn: &Connection,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE mail_project_assignments
         SET project_id = ?1,
             assigned_by = 'user',
             corrected_from = CASE WHEN project_id != ?1 THEN project_id ELSE corrected_from END
         WHERE mail_id = ?2",
        params![project_id, mail_id],
    )?;
    if affected == 0 {
        return Err(AppError::MailNotFound(mail_id.to_string()));
    }
    Ok(())
}

pub fn reject_classification(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM mail_project_assignments WHERE mail_id = ?1",
        params![mail_id],
    )?;
    Ok(())
}

pub fn get_unclassified_mails(
    conn: &Connection,
    account_id: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.account_id, m.folder, m.message_id, m.in_reply_to, m.\"references\",
                m.from_addr, m.to_addr, m.cc_addr, m.subject, m.body_text, m.body_html,
                m.date, m.has_attachments, m.raw_size, m.uid, m.flags, m.fetched_at
         FROM mails m
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE m.account_id = ?1 AND mpa.mail_id IS NULL
         ORDER BY m.date DESC",
    )?;
    let mails = stmt
        .query_map(params![account_id], |row| {
            Ok(Mail {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder: row.get(2)?,
                message_id: row.get(3)?,
                in_reply_to: row.get(4)?,
                references: row.get(5)?,
                from_addr: row.get(6)?,
                to_addr: row.get(7)?,
                cc_addr: row.get(8)?,
                subject: row.get(9)?,
                body_text: row.get(10)?,
                body_html: row.get(11)?,
                date: row.get(12)?,
                has_attachments: row.get(13)?,
                raw_size: row.get(14)?,
                uid: row.get(15)?,
                flags: row.get(16)?,
                fetched_at: row.get(17)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(mails)
}

pub fn get_mails_by_project(conn: &Connection, project_id: &str) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.account_id, m.folder, m.message_id, m.in_reply_to, m.\"references\",
                m.from_addr, m.to_addr, m.cc_addr, m.subject, m.body_text, m.body_html,
                m.date, m.has_attachments, m.raw_size, m.uid, m.flags, m.fetched_at
         FROM mails m
         JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.project_id = ?1
         ORDER BY m.date DESC",
    )?;
    let mails = stmt
        .query_map(params![project_id], |row| {
            Ok(Mail {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder: row.get(2)?,
                message_id: row.get(3)?,
                in_reply_to: row.get(4)?,
                references: row.get(5)?,
                from_addr: row.get(6)?,
                to_addr: row.get(7)?,
                cc_addr: row.get(8)?,
                subject: row.get(9)?,
                body_text: row.get(10)?,
                body_html: row.get(11)?,
                date: row.get(12)?,
                has_attachments: row.get(13)?,
                raw_size: row.get(14)?,
                uid: row.get(15)?,
                flags: row.get(16)?,
                fetched_at: row.get(17)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(mails)
}

pub fn get_recent_subjects(
    conn: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.subject FROM mails m
         JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.project_id = ?1
         ORDER BY m.date DESC LIMIT ?2",
    )?;
    let subjects = stmt
        .query_map(params![project_id, limit], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(subjects)
}

pub fn get_assignment_info(
    conn: &Connection,
    mail_id: &str,
) -> Result<Option<(String, String, Option<f64>)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT project_id, assigned_by, confidence
         FROM mail_project_assignments WHERE mail_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![mail_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    match rows.next() {
        Some(Ok(info)) => Ok(Some(info)),
        Some(Err(e)) => Err(AppError::Database(e)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{accounts, mails, migrations::run_migrations, projects};
    use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest};
    use crate::models::project::CreateProjectRequest;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();
        accounts::insert_account_with_id(
            &conn,
            "acc1",
            &CreateAccountRequest {
                name: "Test".into(),
                email: "test@example.com".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 587,
                auth_type: AuthType::Plain,
                provider: AccountProvider::Other,
                password: None,
            },
        ).unwrap();
        conn
    }

    fn insert_test_mail(conn: &Connection, id: &str, subject: &str) {
        let mail = crate::models::mail::Mail {
            id: id.into(),
            account_id: "acc1".into(),
            folder: "INBOX".into(),
            message_id: format!("<{}@ex.com>", id),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "test@example.com".into(),
            cc_addr: None,
            subject: subject.into(),
            body_text: Some("Body text".into()),
            body_html: None,
            date: "2026-04-13T10:00:00".into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        };
        mails::insert_mail(conn, &mail).unwrap();
    }

    #[test]
    fn test_assign_and_get_by_project() {
        let conn = setup_db();
        let project = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Test Project".into(),
            description: None,
            color: None,
        }).unwrap();
        insert_test_mail(&conn, "m1", "Test Mail");
        assign_mail(&conn, "m1", &project.id, "ai", Some(0.85)).unwrap();
        let mails = get_mails_by_project(&conn, &project.id).unwrap();
        assert_eq!(mails.len(), 1);
        assert_eq!(mails[0].subject, "Test Mail");
    }

    #[test]
    fn test_unclassified_mails() {
        let conn = setup_db();
        let project = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project".into(),
            description: None,
            color: None,
        }).unwrap();
        insert_test_mail(&conn, "m1", "Classified");
        insert_test_mail(&conn, "m2", "Unclassified");
        assign_mail(&conn, "m1", &project.id, "ai", Some(0.9)).unwrap();
        let unclassified = get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
        assert_eq!(unclassified[0].id, "m2");
    }

    #[test]
    fn test_approve_classification_same_project() {
        let conn = setup_db();
        let project = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project".into(),
            description: None,
            color: None,
        }).unwrap();
        insert_test_mail(&conn, "m1", "Mail");
        assign_mail(&conn, "m1", &project.id, "ai", Some(0.5)).unwrap();
        approve_classification(&conn, "m1", &project.id).unwrap();
        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.1, "user");
    }

    #[test]
    fn test_approve_classification_different_project() {
        let conn = setup_db();
        let p1 = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project A".into(),
            description: None,
            color: None,
        }).unwrap();
        let p2 = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project B".into(),
            description: None,
            color: None,
        }).unwrap();
        insert_test_mail(&conn, "m1", "Mail");
        assign_mail(&conn, "m1", &p1.id, "ai", Some(0.5)).unwrap();
        approve_classification(&conn, "m1", &p2.id).unwrap();
        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, p2.id); // project changed
        assert_eq!(info.1, "user");
    }

    #[test]
    fn test_reject_classification() {
        let conn = setup_db();
        let project = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project".into(),
            description: None,
            color: None,
        }).unwrap();
        insert_test_mail(&conn, "m1", "Mail");
        assign_mail(&conn, "m1", &project.id, "ai", Some(0.5)).unwrap();
        reject_classification(&conn, "m1").unwrap();
        let unclassified = get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
    }

    #[test]
    fn test_recent_subjects() {
        let conn = setup_db();
        let project = projects::insert_project(&conn, &CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project".into(),
            description: None,
            color: None,
        }).unwrap();
        for i in 1..=5 {
            let id = format!("m{}", i);
            let subject = format!("Subject {}", i);
            insert_test_mail(&conn, &id, &subject);
            assign_mail(&conn, &id, &project.id, "ai", Some(0.8)).unwrap();
        }
        let subjects = get_recent_subjects(&conn, &project.id, 3).unwrap();
        assert_eq!(subjects.len(), 3);
    }
}
```

- [ ] **Step 2: db/mod.rs にモジュール登録**

```rust
pub mod accounts;
pub mod assignments;
pub mod mails;
pub mod migrations;
pub mod projects;
```

- [ ] **Step 3: テストを実行して PASS を確認**

Run: `cd src-tauri && cargo test db::assignments -- --nocapture`
Expected: ALL PASS

- [ ] **Step 4: コミット**

```bash
git add src-tauri/src/db/assignments.rs src-tauri/src/db/mod.rs
git commit -m "feat(db): mail_project_assignments CRUD（割り当て・承認・却下・未分類取得）"
```

---

## Task 4: Classifier 型定義 + LlmClassifier trait

**Files:**
- Create: `src-tauri/src/models/classifier.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Create: `src-tauri/src/classifier/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/error.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: async-trait を Cargo.toml に追加**

`src-tauri/Cargo.toml` の `[dependencies]` に追加:

```toml
async-trait = "0.1"
```

- [ ] **Step 2: Classifier 型定義を作成**

`src-tauri/src/models/classifier.rs`:

```rust
use serde::{Deserialize, Serialize};

pub const CONFIDENCE_AUTO_ASSIGN: f64 = 0.7;
pub const CONFIDENCE_UNCERTAIN: f64 = 0.4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailSummary {
    pub subject: String,
    pub from_addr: String,
    pub date: String,
    pub body_preview: String,
}

impl MailSummary {
    pub fn from_mail(mail: &crate::models::mail::Mail) -> Self {
        let body_preview = mail
            .body_text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        Self {
            subject: mail.subject.clone(),
            from_addr: mail.from_addr.clone(),
            date: mail.date.clone(),
            body_preview,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub recent_subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionEntry {
    pub mail_subject: String,
    pub from_project: Option<String>,
    pub to_project: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ClassifyAction {
    #[serde(rename = "assign")]
    Assign { project_id: String },
    #[serde(rename = "create")]
    Create {
        project_name: String,
        description: String,
    },
    #[serde(rename = "unclassified")]
    Unclassified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResult {
    #[serde(flatten)]
    pub action: ClassifyAction,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResponse {
    pub mail_id: String,
    #[serde(flatten)]
    pub result: ClassifyResult,
}
```

- [ ] **Step 3: models/mod.rs にモジュール登録**

```rust
pub mod account;
pub mod classifier;
pub mod mail;
pub mod project;
```

- [ ] **Step 4: AppError に Classifier 関連エラーを追加**

`src-tauri/src/error.rs` に追加:

```rust
#[error("Classifier error: {0}")]
Classifier(String),

#[error("Ollama connection failed: {0}")]
OllamaConnection(String),

#[error("Invalid LLM response: {0}")]
InvalidLlmResponse(String),
```

- [ ] **Step 5: LlmClassifier trait を作成**

`src-tauri/src/classifier/mod.rs`:

```rust
pub mod ollama;
pub mod prompt;

use async_trait::async_trait;

use crate::error::AppError;
use crate::models::classifier::{
    ClassifyResult, CorrectionEntry, MailSummary, ProjectSummary,
};

#[async_trait]
pub trait LlmClassifier: Send + Sync {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError>;

    async fn health_check(&self) -> Result<(), AppError>;
}
```

- [ ] **Step 6: lib.rs にモジュール登録**

`src-tauri/src/lib.rs` の先頭に追加:

```rust
pub mod classifier;
```

- [ ] **Step 7: ビルドが通ることを確認**

Run: `cd src-tauri && cargo check`
Expected: OK (ollama.rs と prompt.rs は次タスクで作成)

Note: `ollama.rs` と `prompt.rs` が存在しない場合、`classifier/mod.rs` の `pub mod ollama; pub mod prompt;` を一時的にコメントアウトしてビルドを通す。次タスクで有効化する。

- [ ] **Step 8: コミット**

```bash
git add src-tauri/src/models/classifier.rs src-tauri/src/models/mod.rs \
        src-tauri/src/classifier/mod.rs src-tauri/src/error.rs \
        src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(classifier): ClassifyResult 型定義と LlmClassifier trait を追加"
```

---

## Task 5: プロンプト構築 + OllamaClassifier

**Files:**
- Create: `src-tauri/src/classifier/prompt.rs`
- Create: `src-tauri/src/classifier/ollama.rs`
- Modify: `src-tauri/src/classifier/mod.rs` (pub mod のコメントアウトを解除)

- [ ] **Step 1: プロンプト構築のテストを書く**

`src-tauri/src/classifier/prompt.rs`:

```rust
use crate::models::classifier::{CorrectionEntry, MailSummary, ProjectSummary};

pub const SYSTEM_PROMPT: &str = r#"You are an email classifier. Given an email and a list of existing projects,
determine which project the email belongs to.

Respond with ONLY a JSON object in one of these formats:

1. Assign to existing project:
{"action": "assign", "project_id": "<id>", "confidence": 0.85, "reason": "..."}

2. Propose new project:
{"action": "create", "project_name": "<name>", "description": "<desc>", "confidence": 0.78, "reason": "..."}

3. Cannot classify:
{"action": "unclassified", "confidence": 0.30, "reason": "..."}

Rules:
- confidence is a float between 0.0 and 1.0
- reason is a brief explanation in Japanese
- When no existing project matches well, use "create" to propose a new one
- Use "unclassified" only when the email content is too ambiguous to classify"#;

pub fn build_user_prompt(
    mail: &MailSummary,
    projects: &[ProjectSummary],
    corrections: &[CorrectionEntry],
) -> String {
    let mut prompt = String::new();

    prompt.push_str("## Existing Projects\n");
    if projects.is_empty() {
        prompt.push_str("(No projects yet)\n");
    } else {
        for p in projects {
            let desc = p.description.as_deref().unwrap_or("N/A");
            let subjects = if p.recent_subjects.is_empty() {
                "N/A".to_string()
            } else {
                p.recent_subjects.join(", ")
            };
            prompt.push_str(&format!(
                "- ID: {} | Name: {} | Description: {} | Recent subjects: {}\n",
                p.id, p.name, desc, subjects
            ));
        }
    }

    prompt.push_str("\n## Email to Classify\n");
    prompt.push_str(&format!("- Subject: {}\n", mail.subject));
    prompt.push_str(&format!("- From: {}\n", mail.from_addr));
    prompt.push_str(&format!("- Date: {}\n", mail.date));
    prompt.push_str(&format!("- Body (first 300 chars): {}\n", mail.body_preview));

    if !corrections.is_empty() {
        prompt.push_str("\n## Recent Corrections (for reference)\n");
        for c in corrections {
            let from = c.from_project.as_deref().unwrap_or("(none)");
            prompt.push_str(&format!(
                "- Email \"{}\" was moved from \"{}\" to \"{}\"\n",
                c.mail_subject, from, c.to_project
            ));
        }
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_user_prompt_with_projects() {
        let mail = MailSummary {
            subject: "見積もりの件".into(),
            from_addr: "tanaka@example.com".into(),
            date: "2026-04-13".into(),
            body_preview: "お世話になっております。".into(),
        };
        let projects = vec![ProjectSummary {
            id: "p1".into(),
            name: "案件A".into(),
            description: Some("テスト案件".into()),
            recent_subjects: vec!["前回の件".into(), "確認依頼".into()],
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);
        assert!(prompt.contains("ID: p1"));
        assert!(prompt.contains("Name: 案件A"));
        assert!(prompt.contains("Subject: 見積もりの件"));
        assert!(prompt.contains("前回の件, 確認依頼"));
    }

    #[test]
    fn test_build_user_prompt_no_projects() {
        let mail = MailSummary {
            subject: "Test".into(),
            from_addr: "a@b.com".into(),
            date: "2026-04-13".into(),
            body_preview: "Hello".into(),
        };
        let prompt = build_user_prompt(&mail, &[], &[]);
        assert!(prompt.contains("(No projects yet)"));
    }

    #[test]
    fn test_build_user_prompt_with_corrections() {
        let mail = MailSummary {
            subject: "Test".into(),
            from_addr: "a@b.com".into(),
            date: "2026-04-13".into(),
            body_preview: "Hello".into(),
        };
        let corrections = vec![CorrectionEntry {
            mail_subject: "Old mail".into(),
            from_project: Some("Project A".into()),
            to_project: "Project B".into(),
        }];
        let prompt = build_user_prompt(&mail, &[], &corrections);
        assert!(prompt.contains("Recent Corrections"));
        assert!(prompt.contains("\"Old mail\" was moved from \"Project A\" to \"Project B\""));
    }
}
```

- [ ] **Step 2: テストを実行して PASS を確認**

Run: `cd src-tauri && cargo test classifier::prompt -- --nocapture`
Expected: ALL PASS

- [ ] **Step 3: OllamaClassifier を実装**

`src-tauri/src/classifier/ollama.rs`:

```rust
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::prompt;
use super::LlmClassifier;
use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyResult, CorrectionEntry, MailSummary, ProjectSummary,
};

pub struct OllamaClassifier {
    endpoint: String,
    model: String,
    client: Client,
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
}

impl OllamaClassifier {
    pub fn new(endpoint: String, model: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            endpoint,
            model,
            client,
        }
    }

    fn parse_response(content: &str) -> Result<ClassifyResult, AppError> {
        // Try to extract JSON from the response (LLM may include extra text)
        let json_str = extract_json(content)
            .ok_or_else(|| AppError::InvalidLlmResponse(format!("No JSON found in: {}", content)))?;

        serde_json::from_str::<ClassifyResult>(json_str).map_err(|e| {
            AppError::InvalidLlmResponse(format!("JSON parse error: {} in: {}", e, json_str))
        })
    }
}

fn extract_json(text: &str) -> Option<&str> {
    // Find the first { and last } to extract JSON
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if start <= end {
        Some(&text[start..=end])
    } else {
        None
    }
}

#[async_trait]
impl LlmClassifier for OllamaClassifier {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError> {
        let user_prompt = prompt::build_user_prompt(mail, projects, corrections);

        let request = OllamaChatRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system".into(),
                    content: prompt::SYSTEM_PROMPT.into(),
                },
                OllamaMessage {
                    role: "user".into(),
                    content: user_prompt,
                },
            ],
            stream: false,
        };

        let url = format!("{}/api/chat", self.endpoint);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::OllamaConnection(format!(
                "Ollama returned {}: {}",
                status, body
            )));
        }

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| AppError::InvalidLlmResponse(e.to_string()))?;

        match Self::parse_response(&chat_response.message.content) {
            Ok(result) => Ok(result),
            Err(_) => {
                // Fallback to unclassified on parse failure
                Ok(ClassifyResult {
                    action: ClassifyAction::Unclassified,
                    confidence: 0.0,
                    reason: "LLMレスポンスの解析に失敗しました".into(),
                })
            }
        }
    }

    async fn health_check(&self) -> Result<(), AppError> {
        let url = format!("{}/api/tags", self.endpoint);
        self.client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| {
                AppError::OllamaConnection(
                    "Ollamaが起動していません。`ollama serve` を実行してください".into(),
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_assign() {
        let content = r#"{"action": "assign", "project_id": "p1", "confidence": 0.85, "reason": "件名が一致"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { ref project_id } if project_id == "p1"));
        assert_eq!(result.confidence, 0.85);
    }

    #[test]
    fn test_parse_response_create() {
        let content = r#"{"action": "create", "project_name": "新案件", "description": "新しい案件", "confidence": 0.7, "reason": "新規"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Create { ref project_name, .. } if project_name == "新案件"));
    }

    #[test]
    fn test_parse_response_unclassified() {
        let content = r#"{"action": "unclassified", "confidence": 0.2, "reason": "判定不能"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Unclassified));
    }

    #[test]
    fn test_parse_response_with_surrounding_text() {
        let content = r#"Here is my analysis: {"action": "assign", "project_id": "p1", "confidence": 0.9, "reason": "test"} That's my answer."#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
    }

    #[test]
    fn test_parse_response_invalid() {
        let content = "I don't know how to classify this email.";
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_json() {
        assert_eq!(extract_json(r#"text {"a": 1} end"#), Some(r#"{"a": 1}"#));
        assert_eq!(extract_json("no json here"), None);
    }
}
```

- [ ] **Step 4: classifier/mod.rs の pub mod コメントアウトを解除**

`src-tauri/src/classifier/mod.rs` が `pub mod ollama;` と `pub mod prompt;` を含むことを確認。

- [ ] **Step 5: テストを実行して PASS を確認**

Run: `cd src-tauri && cargo test classifier -- --nocapture`
Expected: ALL PASS

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/classifier/
git commit -m "feat(classifier): プロンプト構築と OllamaClassifier を実装"
```

---

## Task 6: Project Commands + Classify Commands

**Files:**
- Create: `src-tauri/src/commands/project_commands.rs`
- Create: `src-tauri/src/commands/classify_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: project_commands を作成**

`src-tauri/src/commands/project_commands.rs`:

```rust
use tauri::State;

use crate::commands::account_commands::DbState;
use crate::db::projects;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};

#[tauri::command]
pub fn create_project(
    state: State<DbState>,
    account_id: String,
    name: String,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let req = CreateProjectRequest {
        account_id,
        name,
        description,
        color,
    };
    projects::insert_project(&conn, &req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_projects(state: State<DbState>, account_id: String) -> Result<Vec<Project>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    projects::list_projects(&conn, &account_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_project(
    state: State<DbState>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let req = UpdateProjectRequest {
        name,
        description,
        color,
    };
    projects::update_project(&conn, &id, &req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    projects::archive_project(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    projects::delete_project(&conn, &id).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: classify_commands を作成**

`src-tauri/src/commands/classify_commands.rs`:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

use crate::classifier::ollama::OllamaClassifier;
use crate::classifier::LlmClassifier;
use crate::commands::account_commands::DbState;
use crate::db::{assignments, projects};
use crate::models::classifier::{
    ClassifyAction, ClassifyResponse, ClassifyResult, MailSummary, ProjectSummary,
    CONFIDENCE_AUTO_ASSIGN, CONFIDENCE_UNCERTAIN,
};
use crate::models::project::{CreateProjectRequest, Project};

pub struct PendingClassification {
    pub result: ClassifyResult,
}

pub struct PendingClassifications(pub Mutex<HashMap<String, PendingClassification>>);

impl PendingClassifications {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

pub struct ClassifyCancelFlag(pub Arc<AtomicBool>);

impl ClassifyCancelFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

fn get_settings_or_default(conn: &rusqlite::Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get(0),
    )
    .unwrap_or_else(|_| default.to_string())
}

fn build_project_summaries(
    conn: &rusqlite::Connection,
    account_id: &str,
) -> Result<Vec<ProjectSummary>, String> {
    let project_list =
        projects::list_projects(conn, account_id).map_err(|e| e.to_string())?;
    let mut summaries = Vec::new();
    for p in &project_list {
        let recent_subjects =
            assignments::get_recent_subjects(conn, &p.id, 3).map_err(|e| e.to_string())?;
        summaries.push(ProjectSummary {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            recent_subjects,
        });
    }
    Ok(summaries)
}

#[tauri::command]
pub async fn classify_mail(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    mail_id: String,
) -> Result<ClassifyResponse, String> {
    let (mail, project_summaries, endpoint, model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mails = conn
            .prepare(
                "SELECT id, account_id, folder, message_id, in_reply_to, \"references\",
                        from_addr, to_addr, cc_addr, subject, body_text, body_html,
                        date, has_attachments, raw_size, uid, flags, fetched_at
                 FROM mails WHERE id = ?1",
            )
            .map_err(|e| e.to_string())?
            .query_row(rusqlite::params![mail_id], |row| {
                Ok(crate::models::mail::Mail {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    folder: row.get(2)?,
                    message_id: row.get(3)?,
                    in_reply_to: row.get(4)?,
                    references: row.get(5)?,
                    from_addr: row.get(6)?,
                    to_addr: row.get(7)?,
                    cc_addr: row.get(8)?,
                    subject: row.get(9)?,
                    body_text: row.get(10)?,
                    body_html: row.get(11)?,
                    date: row.get(12)?,
                    has_attachments: row.get(13)?,
                    raw_size: row.get(14)?,
                    uid: row.get(15)?,
                    flags: row.get(16)?,
                    fetched_at: row.get(17)?,
                })
            })
            .map_err(|_| format!("Mail not found: {}", mail_id))?;

        let summaries = build_project_summaries(&conn, &mails.account_id)?;
        let endpoint = get_settings_or_default(&conn, "ollama_endpoint", "http://localhost:11434");
        let model = get_settings_or_default(&conn, "ollama_model", "llama3.1:8b");
        (mails, summaries, endpoint, model)
    };

    let classifier = OllamaClassifier::new(endpoint, model);
    classifier.health_check().await.map_err(|e| e.to_string())?;

    let mail_summary = MailSummary::from_mail(&mail);
    let result = classifier
        .classify(&mail_summary, &project_summaries, &[])
        .await
        .map_err(|e| e.to_string())?;

    // Apply result based on confidence
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        match &result.action {
            ClassifyAction::Assign { project_id } if result.confidence >= CONFIDENCE_UNCERTAIN => {
                assignments::assign_mail(&conn, &mail_id, project_id, "ai", Some(result.confidence))
                    .map_err(|e| e.to_string())?;
            }
            ClassifyAction::Create { .. } => {
                let mut pendings = pending.0.lock().map_err(|e| e.to_string())?;
                pendings.insert(
                    mail_id.clone(),
                    PendingClassification {
                        result: result.clone(),
                    },
                );
            }
            _ => {} // Unclassified or low confidence - do nothing
        }
    }

    Ok(ClassifyResponse {
        mail_id,
        result,
    })
}

#[tauri::command]
pub async fn classify_unassigned(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    cancel_flag: State<'_, ClassifyCancelFlag>,
    handle: tauri::AppHandle,
    account_id: String,
) -> Result<(), String> {
    cancel_flag.0.store(false, Ordering::SeqCst);

    let (unclassified, project_summaries, endpoint, model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let unclassified =
            assignments::get_unclassified_mails(&conn, &account_id).map_err(|e| e.to_string())?;
        let summaries = build_project_summaries(&conn, &account_id)?;
        let endpoint = get_settings_or_default(&conn, "ollama_endpoint", "http://localhost:11434");
        let model = get_settings_or_default(&conn, "ollama_model", "llama3.1:8b");
        (unclassified, summaries, endpoint, model)
    };

    let classifier = OllamaClassifier::new(endpoint, model);
    classifier.health_check().await.map_err(|e| e.to_string())?;

    let total = unclassified.len();
    let mut assigned = 0u32;
    let mut needs_review = 0u32;
    let mut unclassified_count = 0u32;

    for (i, mail) in unclassified.iter().enumerate() {
        if cancel_flag.0.load(Ordering::SeqCst) {
            break;
        }

        let mail_summary = MailSummary::from_mail(mail);
        let result = classifier
            .classify(&mail_summary, &project_summaries, &[])
            .await
            .unwrap_or_else(|_| ClassifyResult {
                action: ClassifyAction::Unclassified,
                confidence: 0.0,
                reason: "分類に失敗しました".into(),
            });

        // Apply result
        {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            match &result.action {
                ClassifyAction::Assign { project_id }
                    if result.confidence >= CONFIDENCE_UNCERTAIN =>
                {
                    assignments::assign_mail(
                        &conn,
                        &mail.id,
                        project_id,
                        "ai",
                        Some(result.confidence),
                    )
                    .map_err(|e| e.to_string())?;
                    if result.confidence >= CONFIDENCE_AUTO_ASSIGN {
                        assigned += 1;
                    } else {
                        needs_review += 1;
                    }
                }
                ClassifyAction::Create { .. } => {
                    let mut pendings = pending.0.lock().map_err(|e| e.to_string())?;
                    pendings.insert(
                        mail.id.clone(),
                        PendingClassification {
                            result: result.clone(),
                        },
                    );
                    needs_review += 1;
                }
                _ => {
                    unclassified_count += 1;
                }
            }
        }

        let response = ClassifyResponse {
            mail_id: mail.id.clone(),
            result: result.clone(),
        };
        let _ = handle.emit("classify-progress", serde_json::json!({
            "current": i + 1,
            "total": total,
            "mail_id": mail.id,
            "result": response,
        }));
    }

    let _ = handle.emit("classify-complete", serde_json::json!({
        "total": total,
        "assigned": assigned,
        "needs_review": needs_review,
        "unclassified": unclassified_count,
    }));

    Ok(())
}

#[tauri::command]
pub fn cancel_classification(cancel_flag: State<ClassifyCancelFlag>) -> Result<(), String> {
    cancel_flag.0.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn approve_classification(
    db: State<DbState>,
    mail_id: String,
    project_id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::approve_classification(&conn, &mail_id, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn approve_new_project(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_name: String,
    description: Option<String>,
) -> Result<Project, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Get account_id from the mail
    let account_id: String = conn
        .query_row(
            "SELECT account_id FROM mails WHERE id = ?1",
            rusqlite::params![mail_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("Mail not found: {}", mail_id))?;

    // Transaction: create project + assign mail
    conn.execute_batch("BEGIN TRANSACTION").map_err(|e| e.to_string())?;

    let result = (|| -> Result<Project, String> {
        let req = CreateProjectRequest {
            account_id,
            name: project_name,
            description,
            color: None,
        };
        let project = projects::insert_project(&conn, &req).map_err(|e| e.to_string())?;
        assignments::assign_mail(&conn, &mail_id, &project.id, "user", Some(1.0))
            .map_err(|e| e.to_string())?;
        Ok(project)
    })();

    match result {
        Ok(project) => {
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            // Remove from pending
            if let Ok(mut pendings) = pending.0.lock() {
                pendings.remove(&mail_id);
            }
            Ok(project)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

#[tauri::command]
pub fn reject_classification(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
) -> Result<(), String> {
    // Remove from pending if exists
    if let Ok(mut pendings) = pending.0.lock() {
        pendings.remove(&mail_id);
    }
    // Remove from assignments if exists
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::reject_classification(&conn, &mail_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_unclassified_mails(
    db: State<DbState>,
    account_id: String,
) -> Result<Vec<crate::models::mail::Mail>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::get_unclassified_mails(&conn, &account_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_mails_by_project(
    db: State<DbState>,
    project_id: String,
) -> Result<Vec<crate::models::mail::Mail>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignments::get_mails_by_project(&conn, &project_id).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: commands/mod.rs にモジュール登録**

```rust
pub mod account_commands;
pub mod auth_commands;
pub mod classify_commands;
pub mod mail_commands;
pub mod project_commands;
```

- [ ] **Step 4: lib.rs に State と invoke_handler を登録**

`src-tauri/src/lib.rs` の `tauri::Builder` セクションを更新:

```rust
.manage(commands::classify_commands::PendingClassifications::new())
.manage(commands::classify_commands::ClassifyCancelFlag::new())
```

`invoke_handler` に追加:

```rust
commands::project_commands::create_project,
commands::project_commands::get_projects,
commands::project_commands::update_project,
commands::project_commands::archive_project,
commands::project_commands::delete_project,
commands::classify_commands::classify_mail,
commands::classify_commands::classify_unassigned,
commands::classify_commands::cancel_classification,
commands::classify_commands::approve_classification,
commands::classify_commands::approve_new_project,
commands::classify_commands::reject_classification,
commands::classify_commands::get_unclassified_mails,
commands::classify_commands::get_mails_by_project,
```

- [ ] **Step 5: ビルドが通ることを確認**

Run: `cd src-tauri && cargo check`
Expected: OK

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/commands/ src-tauri/src/lib.rs
git commit -m "feat(commands): Project CRUD と分類コマンドを追加"
```

---

## Task 7: フロントエンド型定義 + Zustand ストア

**Files:**
- Create: `src/types/project.ts`
- Create: `src/types/classifier.ts`
- Create: `src/stores/projectStore.ts`
- Create: `src/stores/classifyStore.ts`

- [ ] **Step 1: 型定義を作成**

`src/types/project.ts`:

```typescript
export interface Project {
  id: string;
  account_id: string;
  name: string;
  description: string | null;
  color: string | null;
  is_archived: boolean;
  created_at: string;
  updated_at: string;
}
```

`src/types/classifier.ts`:

```typescript
export interface ClassifyResponse {
  mail_id: string;
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  confidence: number;
  reason: string;
}

export interface ClassifyProgress {
  current: number;
  total: number;
  mail_id: string;
  result: ClassifyResponse;
}

export interface ClassifySummary {
  total: number;
  assigned: number;
  needs_review: number;
  unclassified: number;
}
```

- [ ] **Step 2: projectStore を作成**

`src/stores/projectStore.ts`:

```typescript
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../types/project";

interface ProjectState {
  projects: Project[];
  selectedProjectId: string | null;
  loading: boolean;
  error: string | null;
  fetchProjects: (accountId: string) => Promise<void>;
  createProject: (
    accountId: string,
    name: string,
    description?: string,
    color?: string,
  ) => Promise<Project>;
  updateProject: (
    id: string,
    name?: string,
    description?: string,
    color?: string,
  ) => Promise<void>;
  archiveProject: (id: string) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
  selectProject: (id: string | null) => void;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  selectedProjectId: null,
  loading: false,
  error: null,

  fetchProjects: async (accountId) => {
    set({ loading: true, error: null });
    try {
      const projects = await invoke<Project[]>("get_projects", { accountId });
      set({ projects, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  createProject: async (accountId, name, description, color) => {
    set({ loading: true, error: null });
    try {
      const project = await invoke<Project>("create_project", {
        accountId,
        name,
        description: description ?? null,
        color: color ?? null,
      });
      const projects = await invoke<Project[]>("get_projects", { accountId });
      set({ projects, loading: false });
      return project;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateProject: async (id, name, description, color) => {
    try {
      const updated = await invoke<Project>("update_project", {
        id,
        name: name ?? null,
        description: description ?? null,
        color: color ?? null,
      });
      set((state) => ({
        projects: state.projects.map((p) => (p.id === id ? updated : p)),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  archiveProject: async (id) => {
    try {
      await invoke("archive_project", { id });
      set((state) => ({
        projects: state.projects.filter((p) => p.id !== id),
        selectedProjectId:
          state.selectedProjectId === id ? null : state.selectedProjectId,
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteProject: async (id) => {
    try {
      await invoke("delete_project", { id });
      set((state) => ({
        projects: state.projects.filter((p) => p.id !== id),
        selectedProjectId:
          state.selectedProjectId === id ? null : state.selectedProjectId,
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  selectProject: (id) => set({ selectedProjectId: id }),
}));
```

- [ ] **Step 3: classifyStore を作成**

`src/stores/classifyStore.ts`:

```typescript
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Mail } from "../types/mail";
import type {
  ClassifyResponse,
  ClassifyProgress,
  ClassifySummary,
} from "../types/classifier";

interface ClassifyState {
  classifying: boolean;
  progress: { current: number; total: number } | null;
  results: ClassifyResponse[];
  summary: ClassifySummary | null;
  unclassifiedMails: Mail[];
  error: string | null;
  fetchUnclassified: (accountId: string) => Promise<void>;
  classifyMail: (mailId: string) => Promise<ClassifyResponse>;
  classifyAll: (accountId: string) => Promise<void>;
  cancelClassification: () => Promise<void>;
  approveClassification: (mailId: string, projectId: string) => Promise<void>;
  approveNewProject: (
    mailId: string,
    projectName: string,
    description?: string,
  ) => Promise<void>;
  rejectClassification: (mailId: string) => Promise<void>;
  initClassifyListeners: () => Promise<() => void>;
}

export const useClassifyStore = create<ClassifyState>((set, get) => ({
  classifying: false,
  progress: null,
  results: [],
  summary: null,
  unclassifiedMails: [],
  error: null,

  fetchUnclassified: async (accountId) => {
    try {
      const mails = await invoke<Mail[]>("get_unclassified_mails", {
        accountId,
      });
      set({ unclassifiedMails: mails });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  classifyMail: async (mailId) => {
    try {
      const result = await invoke<ClassifyResponse>("classify_mail", {
        mailId,
      });
      set((state) => ({
        results: [...state.results, result],
        unclassifiedMails: state.unclassifiedMails.filter(
          (m) => m.id !== mailId,
        ),
      }));
      return result;
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  classifyAll: async (accountId) => {
    set({ classifying: true, progress: null, results: [], summary: null, error: null });
    try {
      await invoke("classify_unassigned", { accountId });
    } catch (e) {
      set({ error: String(e), classifying: false });
    }
  },

  cancelClassification: async () => {
    try {
      await invoke("cancel_classification");
    } catch (e) {
      set({ error: String(e) });
    }
  },

  approveClassification: async (mailId, projectId) => {
    try {
      await invoke("approve_classification", { mailId, projectId });
      set((state) => ({
        results: state.results.filter((r) => r.mail_id !== mailId),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  approveNewProject: async (mailId, projectName, description) => {
    try {
      await invoke("approve_new_project", {
        mailId,
        projectName,
        description: description ?? null,
      });
      set((state) => ({
        results: state.results.filter((r) => r.mail_id !== mailId),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  rejectClassification: async (mailId) => {
    try {
      await invoke("reject_classification", { mailId });
      set((state) => ({
        results: state.results.filter((r) => r.mail_id !== mailId),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  initClassifyListeners: async () => {
    const unlistenProgress = await listen<ClassifyProgress>(
      "classify-progress",
      (event) => {
        set((state) => ({
          progress: {
            current: event.payload.current,
            total: event.payload.total,
          },
          results: [...state.results, event.payload.result],
        }));
      },
    );

    const unlistenComplete = await listen<ClassifySummary>(
      "classify-complete",
      (event) => {
        set({
          classifying: false,
          summary: event.payload,
          progress: null,
        });
      },
    );

    return () => {
      unlistenProgress();
      unlistenComplete();
    };
  },
}));
```

- [ ] **Step 4: コミット**

```bash
git add src/types/project.ts src/types/classifier.ts \
        src/stores/projectStore.ts src/stores/classifyStore.ts
git commit -m "feat(ui): Project/Classify 型定義と Zustand ストアを追加"
```

---

## Task 8: サイドバーに案件ツリーを追加

**Files:**
- Create: `src/components/sidebar/ProjectTree.tsx`
- Create: `src/components/sidebar/ProjectForm.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`

- [ ] **Step 1: ProjectTree コンポーネントを作成**

`src/components/sidebar/ProjectTree.tsx`:

```tsx
import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useClassifyStore } from "../../stores/classifyStore";

interface ProjectTreeProps {
  onSelectUnclassified: () => void;
}

export function ProjectTree({ onSelectUnclassified }: ProjectTreeProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const { projects, selectedProjectId, fetchProjects, selectProject } =
    useProjectStore();
  const { unclassifiedMails, fetchUnclassified } = useClassifyStore();

  useEffect(() => {
    if (selectedAccountId) {
      fetchProjects(selectedAccountId);
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchProjects, fetchUnclassified]);

  if (!selectedAccountId) return null;

  return (
    <div className="border-t">
      <div className="px-4 py-2 text-xs font-semibold uppercase text-gray-500">
        案件
      </div>
      {projects.map((project) => (
        <button
          key={project.id}
          onClick={() => selectProject(project.id)}
          className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
            selectedProjectId === project.id ? "bg-blue-50 text-blue-700" : ""
          }`}
        >
          <span
            className="mr-2 inline-block h-2 w-2 rounded-full"
            style={{ backgroundColor: project.color ?? "#6b7280" }}
          />
          {project.name}
        </button>
      ))}
      <div className="border-t">
        <button
          onClick={onSelectUnclassified}
          className="w-full px-4 py-2 text-left text-sm hover:bg-gray-100"
        >
          {unclassifiedMails.length > 0 && (
            <span className="mr-1 text-yellow-500">&#9888;</span>
          )}
          未分類
          {unclassifiedMails.length > 0 && (
            <span className="ml-1 text-xs text-gray-400">
              ({unclassifiedMails.length})
            </span>
          )}
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: ProjectForm コンポーネントを作成**

`src/components/sidebar/ProjectForm.tsx`:

```tsx
import { useState } from "react";

interface ProjectFormProps {
  onSubmit: (name: string, description?: string, color?: string) => void;
  onCancel: () => void;
}

export function ProjectForm({ onSubmit, onCancel }: ProjectFormProps) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [color, setColor] = useState("#6b7280");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    onSubmit(name.trim(), description.trim() || undefined, color);
  };

  return (
    <form onSubmit={handleSubmit} className="border-t p-4">
      <div className="mb-2 text-sm font-semibold">案件を作成</div>
      <label className="mb-1 block text-xs text-gray-600">
        案件名
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="mt-0.5 block w-full rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="mb-1 block text-xs text-gray-600">
        説明
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="mt-0.5 block w-full rounded border px-2 py-1 text-sm"
        />
      </label>
      <label className="mb-2 block text-xs text-gray-600">
        色
        <input
          type="color"
          value={color}
          onChange={(e) => setColor(e.target.value)}
          className="mt-0.5 block h-6 w-full cursor-pointer"
        />
      </label>
      <div className="flex gap-2">
        <button
          type="submit"
          className="rounded bg-blue-600 px-3 py-1 text-xs text-white hover:bg-blue-700"
        >
          作成
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="rounded border px-3 py-1 text-xs hover:bg-gray-100"
        >
          キャンセル
        </button>
      </div>
    </form>
  );
}
```

- [ ] **Step 3: Sidebar を更新**

`src/components/sidebar/Sidebar.tsx` を更新して ProjectTree と ProjectForm を組み込む:

```tsx
import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { AccountList } from "./AccountList";
import { AccountForm } from "./AccountForm";
import { ProjectTree } from "./ProjectTree";
import { ProjectForm } from "./ProjectForm";
import type { CreateAccountRequest } from "../../types/account";

interface SidebarProps {
  onViewChange?: (view: "threads" | "unclassified" | "project") => void;
}

export function Sidebar({ onViewChange }: SidebarProps) {
  const {
    accounts,
    selectedAccountId,
    fetchAccounts,
    createAccount,
    selectAccount,
    initDeepLinkListener,
  } = useAccountStore();
  const { createProject } = useProjectStore();
  const [showAccountForm, setShowAccountForm] = useState(false);
  const [showProjectForm, setShowProjectForm] = useState(false);

  useEffect(() => {
    fetchAccounts();
  }, [fetchAccounts]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    initDeepLinkListener().then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [initDeepLinkListener]);

  const handleAccountSubmit = async (req: CreateAccountRequest) => {
    await createAccount(req);
    setShowAccountForm(false);
  };

  const handleProjectSubmit = async (
    name: string,
    description?: string,
    color?: string,
  ) => {
    if (selectedAccountId) {
      await createProject(selectedAccountId, name, description, color);
      setShowProjectForm(false);
    }
  };

  return (
    <aside className="flex h-full w-64 flex-col border-r bg-gray-50">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h1 className="text-lg font-bold">Pigeon</h1>
        <button
          onClick={() => setShowAccountForm(!showAccountForm)}
          className="text-sm text-blue-600 hover:underline"
        >
          {showAccountForm ? "閉じる" : "+ 追加"}
        </button>
      </div>
      {showAccountForm && (
        <AccountForm
          onSubmit={handleAccountSubmit}
          onCancel={() => setShowAccountForm(false)}
        />
      )}
      <div className="flex-1 overflow-y-auto">
        <AccountList
          accounts={accounts}
          selectedId={selectedAccountId}
          onSelect={(id) => {
            selectAccount(id);
            onViewChange?.("threads");
          }}
        />
        <ProjectTree
          onSelectUnclassified={() => onViewChange?.("unclassified")}
        />
      </div>
      {selectedAccountId && (
        <div className="border-t">
          {showProjectForm ? (
            <ProjectForm
              onSubmit={handleProjectSubmit}
              onCancel={() => setShowProjectForm(false)}
            />
          ) : (
            <button
              onClick={() => setShowProjectForm(true)}
              className="w-full px-4 py-2 text-left text-sm text-blue-600 hover:bg-gray-100"
            >
              + 案件を作成
            </button>
          )}
        </div>
      )}
    </aside>
  );
}
```

- [ ] **Step 4: コミット**

```bash
git add src/components/sidebar/ProjectTree.tsx src/components/sidebar/ProjectForm.tsx \
        src/components/sidebar/Sidebar.tsx
git commit -m "feat(ui): サイドバーに案件ツリーと案件作成フォームを追加"
```

---

## Task 9: 未分類メール一覧 + 分類ボタン + 結果表示

**Files:**
- Create: `src/components/thread-list/UnclassifiedList.tsx`
- Create: `src/components/thread-list/ClassifyButton.tsx`
- Create: `src/components/common/ClassifyResultBadge.tsx`
- Create: `src/components/common/NewProjectProposal.tsx`

- [ ] **Step 1: ClassifyResultBadge を作成**

`src/components/common/ClassifyResultBadge.tsx`:

```tsx
interface ClassifyResultBadgeProps {
  confidence: number;
  assignedBy: string;
}

export function ClassifyResultBadge({
  confidence,
  assignedBy,
}: ClassifyResultBadgeProps) {
  if (assignedBy === "user") return null;

  if (confidence >= 0.7) {
    return (
      <span className="rounded bg-green-100 px-1.5 py-0.5 text-xs text-green-700">
        AI
      </span>
    );
  }
  if (confidence >= 0.4) {
    return (
      <span className="rounded bg-yellow-100 px-1.5 py-0.5 text-xs text-yellow-700">
        &#9888; AI
      </span>
    );
  }
  return null;
}
```

- [ ] **Step 2: ClassifyButton を作成**

`src/components/thread-list/ClassifyButton.tsx`:

```tsx
import { useClassifyStore } from "../../stores/classifyStore";

interface ClassifyButtonProps {
  accountId: string;
}

export function ClassifyButton({ accountId }: ClassifyButtonProps) {
  const { classifying, progress, classifyAll, cancelClassification } =
    useClassifyStore();

  if (classifying) {
    return (
      <div className="flex items-center gap-2 border-b px-4 py-2">
        <div className="flex-1">
          <div className="mb-1 text-xs text-gray-500">
            分類中... {progress ? `${progress.current}/${progress.total}` : ""}
          </div>
          {progress && (
            <div className="h-1.5 w-full rounded-full bg-gray-200">
              <div
                className="h-1.5 rounded-full bg-blue-600 transition-all"
                style={{
                  width: `${(progress.current / progress.total) * 100}%`,
                }}
              />
            </div>
          )}
        </div>
        <button
          onClick={cancelClassification}
          className="rounded border px-2 py-1 text-xs hover:bg-gray-100"
        >
          中止
        </button>
      </div>
    );
  }

  return (
    <div className="border-b px-4 py-2">
      <button
        onClick={() => classifyAll(accountId)}
        className="w-full rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700"
      >
        分類する
      </button>
    </div>
  );
}
```

- [ ] **Step 3: NewProjectProposal ダイアログを作成**

`src/components/common/NewProjectProposal.tsx`:

```tsx
import { useState } from "react";

interface NewProjectProposalProps {
  mailId: string;
  suggestedName: string;
  suggestedDescription: string;
  reason: string;
  onApprove: (
    mailId: string,
    name: string,
    description?: string,
  ) => void;
  onReject: (mailId: string) => void;
}

export function NewProjectProposal({
  mailId,
  suggestedName,
  suggestedDescription,
  reason,
  onApprove,
  onReject,
}: NewProjectProposalProps) {
  const [name, setName] = useState(suggestedName);
  const [description, setDescription] = useState(suggestedDescription);

  return (
    <div className="rounded border border-yellow-200 bg-yellow-50 p-3">
      <div className="mb-2 text-xs text-gray-600">{reason}</div>
      <label className="mb-1 block text-xs text-gray-600">
        案件名
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="mt-0.5 block w-full rounded border px-2 py-1 text-sm"
        />
      </label>
      <label className="mb-2 block text-xs text-gray-600">
        説明
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="mt-0.5 block w-full rounded border px-2 py-1 text-sm"
        />
      </label>
      <div className="flex gap-2">
        <button
          onClick={() => onApprove(mailId, name, description || undefined)}
          className="rounded bg-blue-600 px-2 py-1 text-xs text-white hover:bg-blue-700"
        >
          案件を作成
        </button>
        <button
          onClick={() => onReject(mailId)}
          className="rounded border px-2 py-1 text-xs hover:bg-gray-100"
        >
          却下
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: UnclassifiedList を作成**

`src/components/thread-list/UnclassifiedList.tsx`:

```tsx
import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { ClassifyButton } from "./ClassifyButton";
import { NewProjectProposal } from "../common/NewProjectProposal";

export function UnclassifiedList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const {
    unclassifiedMails,
    results,
    summary,
    fetchUnclassified,
    approveNewProject,
    rejectClassification,
    initClassifyListeners,
  } = useClassifyStore();

  useEffect(() => {
    if (selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchUnclassified]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    initClassifyListeners().then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [initClassifyListeners]);

  if (!selectedAccountId) return null;

  // Filter results for "create" proposals
  const createProposals = results.filter((r) => r.action === "create");

  return (
    <div className="h-full overflow-y-auto">
      <div className="border-b px-4 py-2 text-sm font-semibold">
        未分類メール ({unclassifiedMails.length})
      </div>
      <ClassifyButton accountId={selectedAccountId} />
      {summary && (
        <div className="border-b px-4 py-2 text-xs text-gray-500">
          分類完了: {summary.assigned} 件割当、{summary.needs_review} 件要確認、
          {summary.unclassified} 件未分類
        </div>
      )}
      {createProposals.map((r) => (
        <div key={r.mail_id} className="border-b p-2">
          <NewProjectProposal
            mailId={r.mail_id}
            suggestedName={r.project_name ?? ""}
            suggestedDescription={r.description ?? ""}
            reason={r.reason}
            onApprove={approveNewProject}
            onReject={rejectClassification}
          />
        </div>
      ))}
      {unclassifiedMails.map((mail) => (
        <div key={mail.id} className="border-b px-4 py-3">
          <div className="text-sm font-medium">{mail.subject}</div>
          <div className="mt-1 text-xs text-gray-500">{mail.from_addr}</div>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 5: コミット**

```bash
git add src/components/thread-list/UnclassifiedList.tsx \
        src/components/thread-list/ClassifyButton.tsx \
        src/components/common/ClassifyResultBadge.tsx \
        src/components/common/NewProjectProposal.tsx
git commit -m "feat(ui): 未分類メール一覧、分類ボタン、確信度バッジ、新規案件提案ダイアログを追加"
```

---

## Task 10: App.tsx にビュー切り替えを統合

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/components/thread-list/ThreadList.tsx`

- [ ] **Step 1: App.tsx にビューステート追加**

`src/App.tsx`:

```tsx
import { useState } from "react";
import "./App.css";
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { UnclassifiedList } from "./components/thread-list/UnclassifiedList";
import { MailView } from "./components/mail-view/MailView";

type ViewMode = "threads" | "unclassified" | "project";

function App() {
  const [viewMode, setViewMode] = useState<ViewMode>("threads");

  return (
    <div className="flex h-screen">
      <Sidebar onViewChange={setViewMode} />
      <div className="w-80 border-r">
        {viewMode === "unclassified" ? (
          <UnclassifiedList />
        ) : (
          <ThreadList viewMode={viewMode} />
        )}
      </div>
      <div className="flex-1">
        <MailView />
      </div>
    </div>
  );
}

export default App;
```

- [ ] **Step 2: ThreadList を更新して案件ビューに対応**

`src/components/thread-list/ThreadList.tsx`:

```tsx
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useMailStore } from "../../stores/mailStore";
import { ThreadItem } from "./ThreadItem";
import type { Mail } from "../../types/mail";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const { threads, selectedThread, fetchThreads, selectThread } =
    useMailStore();

  useEffect(() => {
    if (viewMode === "project" && selectedProjectId) {
      // Fetch mails by project and build threads
      invoke<Mail[]>("get_mails_by_project", {
        projectId: selectedProjectId,
      }).then((mails) => {
        // Use existing thread building via get_threads is not available for project view
        // For now, show mails as individual threads
        const projectThreads = mails.map((m) => ({
          thread_id: m.message_id,
          subject: m.subject,
          last_date: m.date,
          mail_count: 1,
          from_addrs: [m.from_addr],
          mails: [m],
        }));
        useMailStore.setState({ threads: projectThreads });
      });
    } else if (selectedAccountId) {
      fetchThreads(selectedAccountId, "INBOX");
    }
  }, [selectedAccountId, selectedProjectId, viewMode, fetchThreads]);

  if (!selectedAccountId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        アカウントを選択してください
      </div>
    );
  }
  if (threads.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        メールがありません
      </div>
    );
  }
  return (
    <div className="h-full overflow-y-auto">
      {threads.map((thread) => (
        <ThreadItem
          key={thread.thread_id}
          thread={thread}
          selected={selectedThread?.thread_id === thread.thread_id}
          onClick={() => selectThread(thread)}
        />
      ))}
    </div>
  );
}
```

- [ ] **Step 3: フロントエンドのビルドが通ることを確認**

Run: `cd /Users/h.aiso/Projects/pigeon && pnpm tsc --noEmit`
Expected: OK

- [ ] **Step 4: コミット**

```bash
git add src/App.tsx src/components/thread-list/ThreadList.tsx
git commit -m "feat(ui): App にビュー切り替え統合（INBOX / 案件 / 未分類）"
```

---

## Task 11: 全体ビルド確認 + 手動テスト

**Files:** (none - verification only)

- [ ] **Step 1: Rust テスト全件実行**

Run: `cd src-tauri && cargo test -- --nocapture`
Expected: ALL PASS

- [ ] **Step 2: フロントエンドビルド確認**

Run: `cd /Users/h.aiso/Projects/pigeon && pnpm tsc --noEmit`
Expected: OK

- [ ] **Step 3: Tauri dev 起動確認**

Run: `cd /Users/h.aiso/Projects/pigeon && pnpm tauri dev`
Expected: アプリが起動し、サイドバーに案件ツリーが表示される

- [ ] **Step 4: 手動テスト項目**

1. 案件を手動作成できる
2. 作成した案件がサイドバーに表示される
3. 「未分類」をクリックすると未分類メール一覧が表示される
4. 「分類する」ボタンをクリックするとOllamaに接続を試みる（Ollamaが起動していない場合はエラーメッセージ）
5. Ollamaが起動している場合、分類が実行されてプログレスバーが表示される

- [ ] **Step 5: 最終コミット（必要な修正があれば）**

修正があった場合のみコミット。
