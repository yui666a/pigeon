# 案件ディレクトリ連携（バックエンド）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 案件にローカルディレクトリを紐付け、スキャン→PIGEON-CONTEXT.md 生成→分類プロンプト注入までのバックエンド一式を実装する。

**Architecture:** 新モジュール `src-tauri/src/project_context/`（scanner / extractor / context_file / digest / cloud_policy）+ DB テーブル4つ（migrate_v5）+ Tauri commands。分類ホットパスは DB キャッシュ（`project_contexts.cached_context`）のみ読む。重い処理（走査・抽出・LLM ダイジェスト生成）はスキャン時に実行。

**Tech Stack:** Rust / rusqlite 0.31 / tokio / reqwest（Ollama API）/ sha2 / chrono / uuid / tempfile（テスト）

**Spec:** `docs/superpowers/specs/2026-07-09-project-directory-context-design.md`（このプランの正）

**UI は別プラン:** フロントエンド（紐付けUI・送信設定ダイアログ等）は本プラン完了後に `2026-07-09-project-directory-context-ui.md` として作成する。

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。エラーは `AppError`（thiserror）
- テストは実装より先に書く（Red → Green）。テスト実行はすべて `src-tauri/` で `cargo test`
- コミットは Conventional Commits（scope: `db`, `project-context`, `classifier` 等）
- スキャン上限（スペック§4の値、変更禁止）: 最大走査 **2,000ファイル** / 最大深さ **10** / テキスト抽出 **1ファイル10KB・案件計100KB** / 分類注入 **1案件800字**
- テキスト系拡張子: `.txt .md .csv .json .yaml .yml .html`
- 隠しファイル・シンボリックリンク・`node_modules`・`PIGEON-CONTEXT.md` 自身はスキャン対象外
- クラウド許可判定は「明示的 allow ルールにマッチした場合のみ true。マッチ無し・曖昧 → false」
- PIGEON-CONTEXT.md のマーカー `<!-- pigeon:auto -->` より上（ユーザー欄）には絶対に書き込まない
- 各タスク完了時に `cargo clippy -- -D warnings` が通ること
- PR 分割の目安: PR1 = Task 1–3（DB基盤）、PR2 = Task 4–6（走査・抽出・コンテキストファイル）、PR3 = Task 7–9（送信ポリシー・ダイジェスト）、PR4 = Task 10–12（オーケストレータ・プロンプト注入・commands）。Stacked PR として依存を明記

---

### Task 1: DB マイグレーション v5

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`
- Test: 同ファイル `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: 既存の `run_migrations` / `set_schema_version` パターン（v4 と同形式）
- Produces: テーブル `project_directories` / `project_files` / `project_cloud_rules` / `project_contexts`（スキーマはスペック§2そのまま）

- [ ] **Step 1: 失敗するテストを書く**

`migrations.rs` の tests モジュール末尾に追加:

```rust
#[test]
fn test_v5_migration_creates_directory_tables() {
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

    assert!(tables.contains(&"project_directories".to_string()));
    assert!(tables.contains(&"project_files".to_string()));
    assert!(tables.contains(&"project_cloud_rules".to_string()));
    assert!(tables.contains(&"project_contexts".to_string()));

    let version: i32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 5);
}

#[test]
fn test_v5_cascade_delete_from_project() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'A', 'a@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES ('d1', 'p1', '/tmp/proj1', TRUE)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO project_files (id, directory_id, relative_path, size_bytes, mtime)
         VALUES ('f1', 'd1', 'a.txt', 10, '2026-07-09T00:00:00Z')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO project_cloud_rules (id, directory_id, scope, relative_path, allow)
         VALUES ('r1', 'd1', 'directory', '', TRUE)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO project_contexts (project_id, cached_context) VALUES ('p1', 'ctx')",
        [],
    ).unwrap();

    conn.execute("DELETE FROM projects WHERE id = 'p1'", []).unwrap();

    for table in ["project_directories", "project_files", "project_cloud_rules", "project_contexts"] {
        let count: i32 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "{} should cascade-delete", table);
    }
}

#[test]
fn test_v5_unique_path_prevents_double_link() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
        [],
    ).unwrap();
    conn.execute("INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P1')", []).unwrap();
    conn.execute("INSERT INTO projects (id, account_id, name) VALUES ('p2', 'acc1', 'P2')", []).unwrap();
    conn.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES ('d1', 'p1', '/tmp/shared', TRUE)",
        [],
    ).unwrap();

    // 同じパスを別案件に紐付けると UNIQUE(path) 違反
    let result = conn.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES ('d2', 'p2', '/tmp/shared', TRUE)",
        [],
    );
    assert!(result.is_err(), "same path must not be linked to two projects");
}

#[test]
fn test_v5_one_primary_per_project() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
        [],
    ).unwrap();
    conn.execute("INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P1')", []).unwrap();
    conn.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES ('d1', 'p1', '/tmp/a', TRUE)",
        [],
    ).unwrap();

    // 2つ目の primary は部分ユニークインデックス違反
    let result = conn.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES ('d2', 'p1', '/tmp/b', TRUE)",
        [],
    );
    assert!(result.is_err(), "only one primary directory per project");
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::migrations::tests::test_v5 -- --nocapture`
Expected: FAIL（`no such table: project_directories` / version 4）

- [ ] **Step 3: migrate_v5 を実装**

`migrations.rs` の `migrate_v4` の後に追加し、`run_migrations` に `if version < 5 { migrate_v5(conn)?; version = 5; set_schema_version(conn, version)?; }` ブロックを追加:

```rust
fn migrate_v5(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        -- 案件⇔ディレクトリ (1:N。UIは当面1案件1ディレクトリに制限)
        CREATE TABLE IF NOT EXISTS project_directories (
            id              TEXT PRIMARY KEY,
            project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            path            TEXT NOT NULL UNIQUE,
            is_primary      BOOLEAN NOT NULL DEFAULT FALSE,
            status          TEXT NOT NULL DEFAULT 'ok'
                            CHECK(status IN ('ok','missing','inaccessible','error')),
            last_scanned_at DATETIME,
            created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_project_directories_project
            ON project_directories(project_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_project_directories_one_primary
            ON project_directories(project_id) WHERE is_primary = TRUE;

        -- ファイルインベントリ (現在の実体のスナップショット)
        CREATE TABLE IF NOT EXISTS project_files (
            id             TEXT PRIMARY KEY,
            directory_id   TEXT NOT NULL REFERENCES project_directories(id) ON DELETE CASCADE,
            relative_path  TEXT NOT NULL,
            size_bytes     INTEGER NOT NULL,
            mtime          DATETIME NOT NULL,
            content_hash   TEXT,
            content_kind   TEXT NOT NULL DEFAULT 'none'
                           CHECK(content_kind IN ('none','text','pdf','office','other')),
            extract_status TEXT NOT NULL DEFAULT 'ok'
                           CHECK(extract_status IN ('ok','skipped_too_large','unsupported','error')),
            indexed_at     DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(directory_id, relative_path)
        );
        CREATE INDEX IF NOT EXISTS idx_project_files_directory
            ON project_files(directory_id);

        -- クラウド送信許可ルール (デフォルト不許可、最長マッチ優先)
        CREATE TABLE IF NOT EXISTS project_cloud_rules (
            id            TEXT PRIMARY KEY,
            directory_id  TEXT NOT NULL REFERENCES project_directories(id) ON DELETE CASCADE,
            scope         TEXT NOT NULL CHECK(scope IN ('directory','file')),
            relative_path TEXT NOT NULL DEFAULT '',
            allow         BOOLEAN NOT NULL,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(directory_id, scope, relative_path)
        );
        CREATE INDEX IF NOT EXISTS idx_project_cloud_rules_directory
            ON project_cloud_rules(directory_id);

        -- 案件のAIコンテキスト状態 (正本は PIGEON-CONTEXT.md、これはキャッシュ+メタ)
        CREATE TABLE IF NOT EXISTS project_contexts (
            project_id          TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            cached_context      TEXT,
            context_hash        TEXT,
            inventory_hash      TEXT,
            allow_cloud_context BOOLEAN NOT NULL DEFAULT FALSE,
            generated_at        DATETIME
        );
        ",
    )?;
    Ok(())
}
```

既存テストの `assert_eq!(version, 4)` を検索して `5` に更新する（`test_v3_migration_creates_projects_and_assignments`, `test_v2_migration_on_existing_v1_database`, `test_v4_migration_creates_fts_table` の3箇所）。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test db::migrations -- --nocapture`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/migrations.rs
git commit -m "feat(db): 案件ディレクトリ連携のスキーマを追加 (migrate_v5)"
```

---

### Task 2: モデル定義 + db/directories.rs（紐付けCRUD）

**Files:**
- Create: `src-tauri/src/models/directory.rs`
- Create: `src-tauri/src/db/directories.rs`
- Modify: `src-tauri/src/models/mod.rs`（`pub mod directory;` 追加）
- Modify: `src-tauri/src/db/mod.rs`（`pub mod directories;` 追加）
- Modify: `src-tauri/src/error.rs`（`DirectoryNotFound` 追加）

**Interfaces:**
- Produces（後続タスクが使う型・関数）:
  - `models::directory::ProjectDirectory { id, project_id, path, is_primary, status, last_scanned_at: Option<String>, created_at }`
  - `models::directory::ProjectFile { id, directory_id, relative_path, size_bytes: i64, mtime, content_hash: Option<String>, content_kind, extract_status, indexed_at }`
  - `models::directory::ProjectFileEntry { relative_path, size_bytes: i64, mtime, content_hash: Option<String>, content_kind: String, extract_status: String }`（スキャン結果の1行。DB行になる前の形）
  - `models::directory::CloudRule { id, directory_id, scope, relative_path, allow: bool }`
  - `models::directory::ProjectContext { project_id, cached_context: Option<String>, context_hash: Option<String>, inventory_hash: Option<String>, allow_cloud_context: bool, generated_at: Option<String> }`
  - `db::directories::link_directory(conn, project_id, path) -> Result<ProjectDirectory, AppError>`（既存紐付けは置換）
  - `db::directories::get_directory_by_project(conn, project_id) -> Result<Option<ProjectDirectory>, AppError>`
  - `db::directories::unlink_directory(conn, project_id) -> Result<(), AppError>`
  - `db::directories::set_status(conn, directory_id, status) -> Result<(), AppError>`
  - `db::directories::touch_scanned(conn, directory_id) -> Result<(), AppError>`
  - `AppError::DirectoryNotFound(String)`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/directories.rs` を作成し、テストから書く:

```rust
use crate::error::AppError;
use crate::models::directory::ProjectDirectory;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES (?1, 'acc1', 'Proj')",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn test_link_and_get_directory() {
        let conn = setup_db();
        create_project(&conn, "p1");

        let dir = link_directory(&conn, "p1", "/tmp/stage-a").unwrap();
        assert_eq!(dir.project_id, "p1");
        assert_eq!(dir.path, "/tmp/stage-a");
        assert!(dir.is_primary);
        assert_eq!(dir.status, "ok");

        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert_eq!(fetched.id, dir.id);
    }

    #[test]
    fn test_link_replaces_existing() {
        let conn = setup_db();
        create_project(&conn, "p1");

        let first = link_directory(&conn, "p1", "/tmp/old").unwrap();
        let second = link_directory(&conn, "p1", "/tmp/new").unwrap();
        assert_ne!(first.id, second.id);

        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert_eq!(fetched.path, "/tmp/new");
    }

    #[test]
    fn test_get_directory_none_when_unlinked() {
        let conn = setup_db();
        create_project(&conn, "p1");
        assert!(get_directory_by_project(&conn, "p1").unwrap().is_none());

        link_directory(&conn, "p1", "/tmp/x").unwrap();
        unlink_directory(&conn, "p1").unwrap();
        assert!(get_directory_by_project(&conn, "p1").unwrap().is_none());
    }

    #[test]
    fn test_set_status_and_touch_scanned() {
        let conn = setup_db();
        create_project(&conn, "p1");
        let dir = link_directory(&conn, "p1", "/tmp/x").unwrap();

        set_status(&conn, &dir.id, "missing").unwrap();
        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert_eq!(fetched.status, "missing");
        assert!(fetched.last_scanned_at.is_none());

        touch_scanned(&conn, &dir.id).unwrap();
        let fetched = get_directory_by_project(&conn, "p1").unwrap().unwrap();
        assert!(fetched.last_scanned_at.is_some());
    }

    #[test]
    fn test_set_status_not_found() {
        let conn = setup_db();
        let result = set_status(&conn, "nonexistent", "ok");
        assert!(matches!(result, Err(AppError::DirectoryNotFound(_))));
    }
}
```

`src-tauri/src/models/directory.rs`（テストのコンパイルに必要なので同時に作成）:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDirectory {
    pub id: String,
    pub project_id: String,
    pub path: String,
    pub is_primary: bool,
    pub status: String, // 'ok' | 'missing' | 'inaccessible' | 'error'
    pub last_scanned_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub id: String,
    pub directory_id: String,
    pub relative_path: String,
    pub size_bytes: i64,
    pub mtime: String,
    pub content_hash: Option<String>,
    pub content_kind: String,   // 'none' | 'text' | 'pdf' | 'office' | 'other'
    pub extract_status: String, // 'ok' | 'skipped_too_large' | 'unsupported' | 'error'
    pub indexed_at: String,
}

/// スキャン結果の1ファイル分（DBに入る前の形）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFileEntry {
    pub relative_path: String,
    pub size_bytes: i64,
    pub mtime: String,
    pub content_hash: Option<String>,
    pub content_kind: String,
    pub extract_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudRule {
    pub id: String,
    pub directory_id: String,
    pub scope: String, // 'directory' | 'file'
    pub relative_path: String,
    pub allow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_id: String,
    pub cached_context: Option<String>,
    pub context_hash: Option<String>,
    pub inventory_hash: Option<String>,
    pub allow_cloud_context: bool,
    pub generated_at: Option<String>,
}
```

`models/mod.rs` に `pub mod directory;`、`db/mod.rs` に `pub mod directories;` を追加。`error.rs` の enum に追加:

```rust
    #[error("Directory not found: {0}")]
    DirectoryNotFound(String),
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::directories`
Expected: コンパイルエラー（`link_directory` 未定義）

- [ ] **Step 3: CRUD を実装**

`db/directories.rs` のテストモジュールの上に実装:

```rust
fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectDirectory> {
    Ok(ProjectDirectory {
        id: row.get(0)?,
        project_id: row.get(1)?,
        path: row.get(2)?,
        is_primary: row.get(3)?,
        status: row.get(4)?,
        last_scanned_at: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const SELECT_COLS: &str =
    "id, project_id, path, is_primary, status, last_scanned_at, created_at";

/// 案件にディレクトリを紐付ける。既存の紐付けがあれば置換する。
pub fn link_directory(
    conn: &Connection,
    project_id: &str,
    path: &str,
) -> Result<ProjectDirectory, AppError> {
    conn.execute(
        "DELETE FROM project_directories WHERE project_id = ?1",
        params![project_id],
    )?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO project_directories (id, project_id, path, is_primary)
         VALUES (?1, ?2, ?3, TRUE)",
        params![id, project_id, path],
    )?;
    conn.query_row(
        &format!("SELECT {} FROM project_directories WHERE id = ?1", SELECT_COLS),
        params![id],
        map_row,
    )
    .map_err(AppError::Database)
}

pub fn get_directory_by_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ProjectDirectory>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {} FROM project_directories WHERE project_id = ?1 AND is_primary = TRUE",
            SELECT_COLS
        ),
        params![project_id],
        map_row,
    )
    .optional()
    .map_err(AppError::Database)
}

pub fn unlink_directory(conn: &Connection, project_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM project_directories WHERE project_id = ?1",
        params![project_id],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, directory_id: &str, status: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE project_directories SET status = ?1 WHERE id = ?2",
        params![status, directory_id],
    )?;
    if affected == 0 {
        return Err(AppError::DirectoryNotFound(directory_id.to_string()));
    }
    Ok(())
}

pub fn touch_scanned(conn: &Connection, directory_id: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE project_directories SET last_scanned_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![directory_id],
    )?;
    if affected == 0 {
        return Err(AppError::DirectoryNotFound(directory_id.to_string()));
    }
    Ok(())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test db::directories`
Expected: 5件 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/directory.rs src-tauri/src/models/mod.rs \
        src-tauri/src/db/directories.rs src-tauri/src/db/mod.rs src-tauri/src/error.rs
git commit -m "feat(db): 案件ディレクトリの紐付けCRUDとモデルを追加"
```

---

### Task 3: db/project_files.rs（インベントリ保存）

**Files:**
- Create: `src-tauri/src/db/project_files.rs`
- Modify: `src-tauri/src/db/mod.rs`（`pub mod project_files;` 追加）

**Interfaces:**
- Consumes: `ProjectFileEntry` / `ProjectFile`（Task 2）
- Produces:
  - `db::project_files::replace_inventory(conn: &mut Connection, directory_id: &str, entries: &[ProjectFileEntry]) -> Result<(), AppError>`（トランザクションで全置換。冪等）
  - `db::project_files::list_files(conn, directory_id) -> Result<Vec<ProjectFile>, AppError>`

- [ ] **Step 1: 失敗するテストを書く**

```rust
use crate::error::AppError;
use crate::models::directory::{ProjectFile, ProjectFileEntry};
use rusqlite::{params, Connection};
use uuid::Uuid;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn setup_dir(conn: &Connection) -> String {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(conn, "p1", "/tmp/x")
            .unwrap()
            .id
    }

    fn entry(path: &str, size: i64) -> ProjectFileEntry {
        ProjectFileEntry {
            relative_path: path.to_string(),
            size_bytes: size,
            mtime: "2026-07-09T00:00:00Z".to_string(),
            content_hash: None,
            content_kind: "other".to_string(),
            extract_status: "unsupported".to_string(),
        }
    }

    #[test]
    fn test_replace_inventory_inserts_and_lists() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&conn);

        replace_inventory(&mut conn, &dir_id, &[entry("a.pdf", 100), entry("sub/b.txt", 20)])
            .unwrap();

        let files = list_files(&conn, &dir_id).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].relative_path, "a.pdf"); // relative_path 順
        assert_eq!(files[1].relative_path, "sub/b.txt");
    }

    #[test]
    fn test_replace_inventory_removes_deleted_files() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&conn);

        replace_inventory(&mut conn, &dir_id, &[entry("a.pdf", 100), entry("b.txt", 20)]).unwrap();
        // b.txt が消えた状態で再スキャン
        replace_inventory(&mut conn, &dir_id, &[entry("a.pdf", 100)]).unwrap();

        let files = list_files(&conn, &dir_id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "a.pdf");
    }

    #[test]
    fn test_replace_inventory_empty_is_ok() {
        let mut conn = setup_db();
        let dir_id = setup_dir(&conn);
        replace_inventory(&mut conn, &dir_id, &[]).unwrap();
        assert!(list_files(&conn, &dir_id).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::project_files`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

```rust
/// インベントリをスナップショットとして全置換する（スペック§4: 消えたファイルはハードデリート）。
/// トランザクション内で実行するため途中失敗しても前回の状態が残り、冪等にやり直せる。
pub fn replace_inventory(
    conn: &mut Connection,
    directory_id: &str,
    entries: &[ProjectFileEntry],
) -> Result<(), AppError> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM project_files WHERE directory_id = ?1",
        params![directory_id],
    )?;
    for e in entries {
        tx.execute(
            "INSERT INTO project_files
                (id, directory_id, relative_path, size_bytes, mtime,
                 content_hash, content_kind, extract_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                directory_id,
                e.relative_path,
                e.size_bytes,
                e.mtime,
                e.content_hash,
                e.content_kind,
                e.extract_status,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn list_files(conn: &Connection, directory_id: &str) -> Result<Vec<ProjectFile>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, directory_id, relative_path, size_bytes, mtime,
                content_hash, content_kind, extract_status, indexed_at
         FROM project_files WHERE directory_id = ?1 ORDER BY relative_path",
    )?;
    let files = stmt
        .query_map(params![directory_id], |row| {
            Ok(ProjectFile {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                relative_path: row.get(2)?,
                size_bytes: row.get(3)?,
                mtime: row.get(4)?,
                content_hash: row.get(5)?,
                content_kind: row.get(6)?,
                extract_status: row.get(7)?,
                indexed_at: row.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(files)
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test db::project_files`
Expected: 3件 PASS。あわせて `cargo clippy -- -D warnings` が通ること（PR1 の締め）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/project_files.rs src-tauri/src/db/mod.rs
git commit -m "feat(db): ファイルインベントリの全置換保存を追加"
```

---

### Task 4: project_context/extractor.rs（種別判定・テキスト抽出）

**Files:**
- Create: `src-tauri/src/project_context/mod.rs`（`pub mod extractor;` のみ）
- Create: `src-tauri/src/project_context/extractor.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod project_context;` 追加）

**Interfaces:**
- Produces:
  - `extractor::MAX_EXTRACT_BYTES_PER_FILE: usize = 10 * 1024`
  - `extractor::MAX_EXTRACT_BYTES_PER_PROJECT: usize = 100 * 1024`
  - `extractor::MAX_HASHABLE_FILE_BYTES: u64 = 1024 * 1024`（これを超えるテキストファイルは `skipped_too_large`）
  - `extractor::content_kind_for(path: &Path) -> &'static str`（"text" | "pdf" | "office" | "other"）
  - `extractor::extract_text(path: &Path) -> Result<ExtractedText, AppError>` / `struct ExtractedText { text: String, truncated: bool, hash: String }`
  - `extractor::sha256_hex(bytes: &[u8]) -> String`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn test_content_kind_for() {
        assert_eq!(content_kind_for(Path::new("a.txt")), "text");
        assert_eq!(content_kind_for(Path::new("香盤表.md")), "text");
        assert_eq!(content_kind_for(Path::new("data.CSV")), "text"); // 大文字拡張子
        assert_eq!(content_kind_for(Path::new("平面図.pdf")), "pdf");
        assert_eq!(content_kind_for(Path::new("見積.xlsx")), "office");
        assert_eq!(content_kind_for(Path::new("photo.jpg")), "other");
        assert_eq!(content_kind_for(Path::new("no_extension")), "other");
    }

    #[test]
    fn test_extract_text_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memo.txt");
        std::fs::write(&path, "搬入は9時から").unwrap();

        let extracted = extract_text(&path).unwrap();
        assert_eq!(extracted.text, "搬入は9時から");
        assert!(!extracted.truncated);
        assert_eq!(extracted.hash.len(), 64); // sha256 hex
    }

    #[test]
    fn test_extract_text_truncates_at_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&vec![b'a'; MAX_EXTRACT_BYTES_PER_FILE + 500]).unwrap();

        let extracted = extract_text(&path).unwrap();
        assert!(extracted.truncated);
        assert!(extracted.text.len() <= MAX_EXTRACT_BYTES_PER_FILE);
    }

    #[test]
    fn test_extract_text_same_content_same_hash() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("a.txt");
        let p2 = dir.path().join("b.txt");
        std::fs::write(&p1, "same").unwrap();
        std::fs::write(&p2, "same").unwrap();
        assert_eq!(extract_text(&p1).unwrap().hash, extract_text(&p2).unwrap().hash);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test project_context::extractor`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

```rust
use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub const MAX_EXTRACT_BYTES_PER_FILE: usize = 10 * 1024;
pub const MAX_EXTRACT_BYTES_PER_PROJECT: usize = 100 * 1024;
pub const MAX_HASHABLE_FILE_BYTES: u64 = 1024 * 1024;

const TEXT_EXTENSIONS: &[&str] = &["txt", "md", "csv", "json", "yaml", "yml", "html"];
const OFFICE_EXTENSIONS: &[&str] = &["xlsx", "xls", "docx", "doc", "pptx", "ppt"];

pub struct ExtractedText {
    pub text: String,
    pub truncated: bool,
    pub hash: String,
}

pub fn content_kind_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if TEXT_EXTENSIONS.contains(&ext.as_str()) {
        "text"
    } else if ext == "pdf" {
        "pdf"
    } else if OFFICE_EXTENSIONS.contains(&ext.as_str()) {
        "office"
    } else {
        "other"
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

/// テキストファイルの内容を上限付きで読む。ハッシュは抽出（切詰後）バイト列に対して計算する。
pub fn extract_text(path: &Path) -> Result<ExtractedText, AppError> {
    let file = std::fs::File::open(path)
        .map_err(|e| AppError::DirectoryScan(format!("{}: {}", path.display(), e)))?;
    let mut buf = Vec::with_capacity(MAX_EXTRACT_BYTES_PER_FILE);
    let mut handle = file.take((MAX_EXTRACT_BYTES_PER_FILE + 1) as u64);
    handle
        .read_to_end(&mut buf)
        .map_err(|e| AppError::DirectoryScan(format!("{}: {}", path.display(), e)))?;

    let truncated = buf.len() > MAX_EXTRACT_BYTES_PER_FILE;
    buf.truncate(MAX_EXTRACT_BYTES_PER_FILE);
    let hash = sha256_hex(&buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(ExtractedText { text, truncated, hash })
}
```

`error.rs` に追加（このタスクで使用開始）:

```rust
    #[error("Directory scan error: {0}")]
    DirectoryScan(String),
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test project_context::extractor`
Expected: 4件 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/project_context/ src-tauri/src/lib.rs src-tauri/src/error.rs
git commit -m "feat(project-context): ファイル種別判定とテキスト抽出を追加"
```

---

### Task 5: project_context/scanner.rs（走査 + inventory_hash）

**Files:**
- Create: `src-tauri/src/project_context/scanner.rs`
- Modify: `src-tauri/src/project_context/mod.rs`（`pub mod scanner;` 追加）

**Interfaces:**
- Consumes: `extractor::{content_kind_for, extract_text, MAX_HASHABLE_FILE_BYTES}`、`ProjectFileEntry`
- Produces:
  - `scanner::CONTEXT_FILE_NAME: &str = "PIGEON-CONTEXT.md"`
  - `scanner::MAX_FILES: usize = 2000` / `scanner::MAX_DEPTH: usize = 10`
  - `scanner::scan_directory(root: &Path) -> Result<ScanResult, AppError>`
  - `struct ScanResult { files: Vec<ProjectFileEntry>, inventory_hash: String }`（files は relative_path 昇順）
  - ルートが存在しない → `Err(AppError::DirectoryScan)` で `io::ErrorKind::NotFound` を含む文言（オーケストレータが missing/inaccessible を判別するため `scan_directory` は `ScanError { kind: ScanErrorKind, message }` ではなく、`scanner::classify_io_error(e: &std::io::Error) -> &'static str`（"missing" | "inaccessible" | "error"）を併せて公開する）

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join("図面")).unwrap();
        std::fs::write(dir.join("図面/平面図.pdf"), b"%PDF-fake").unwrap();
        std::fs::write(dir.join("香盤表.md"), "第1幕 くるみ割り").unwrap();
        std::fs::write(dir.join("搬入.txt"), "9時集合").unwrap();
    }

    #[test]
    fn test_scan_directory_basic() {
        let dir = tempfile::tempdir().unwrap();
        make_tree(dir.path());

        let result = scan_directory(dir.path()).unwrap();
        let paths: Vec<&str> = result.files.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(paths, vec!["搬入.txt", "香盤表.md", "図面/平面図.pdf"]
            .into_iter().collect::<std::collections::BTreeSet<_>>()
            .into_iter().collect::<Vec<_>>());

        let md = result.files.iter().find(|f| f.relative_path == "香盤表.md").unwrap();
        assert_eq!(md.content_kind, "text");
        assert_eq!(md.extract_status, "ok");
        assert!(md.content_hash.is_some());

        let pdf = result.files.iter().find(|f| f.relative_path.ends_with("平面図.pdf")).unwrap();
        assert_eq!(pdf.content_kind, "pdf");
        assert_eq!(pdf.extract_status, "unsupported");
        assert!(pdf.content_hash.is_none());
    }

    #[test]
    fn test_scan_skips_hidden_symlink_node_modules_and_context_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::write(dir.path().join("node_modules/pkg/index.js"), b"x").unwrap();
        std::fs::write(dir.path().join(CONTEXT_FILE_NAME), "# ctx").unwrap();
        std::fs::write(dir.path().join("keep.txt"), "keep").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("keep.txt"), dir.path().join("link.txt")).unwrap();

        let result = scan_directory(dir.path()).unwrap();
        let paths: Vec<&str> = result.files.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn test_inventory_hash_stable_and_change_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        make_tree(dir.path());

        let h1 = scan_directory(dir.path()).unwrap().inventory_hash;
        let h2 = scan_directory(dir.path()).unwrap().inventory_hash;
        assert_eq!(h1, h2, "同一構成なら同一ハッシュ");

        std::fs::write(dir.path().join("新資料.txt"), "追加").unwrap();
        let h3 = scan_directory(dir.path()).unwrap().inventory_hash;
        assert_ne!(h1, h3, "ファイル追加でハッシュが変わる");
    }

    #[test]
    fn test_scan_missing_root_is_error() {
        let result = scan_directory(std::path::Path::new("/nonexistent/pigeon-test"));
        assert!(result.is_err());
    }

    #[test]
    fn test_classify_io_error() {
        use std::io::{Error, ErrorKind};
        assert_eq!(classify_io_error(&Error::from(ErrorKind::NotFound)), "missing");
        assert_eq!(classify_io_error(&Error::from(ErrorKind::PermissionDenied)), "inaccessible");
        assert_eq!(classify_io_error(&Error::from(ErrorKind::Other)), "error");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test project_context::scanner`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

```rust
use crate::error::AppError;
use crate::models::directory::ProjectFileEntry;
use crate::project_context::extractor;
use chrono::{DateTime, Utc};
use std::path::Path;

pub const CONTEXT_FILE_NAME: &str = "PIGEON-CONTEXT.md";
pub const MAX_FILES: usize = 2000;
pub const MAX_DEPTH: usize = 10;
const IGNORED_DIRS: &[&str] = &["node_modules", "target", ".git"];

pub struct ScanResult {
    pub files: Vec<ProjectFileEntry>,
    pub inventory_hash: String,
}

pub fn classify_io_error(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "missing",
        std::io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "error",
    }
}

pub fn scan_directory(root: &Path) -> Result<ScanResult, AppError> {
    // ルートの存在確認（io::Error を文言に含め、呼び出し側が classify できるようにする）
    std::fs::read_dir(root)
        .map_err(|e| AppError::DirectoryScan(format!("{} [{}]", e, classify_io_error(&e))))?;

    let mut files = Vec::new();
    walk(root, root, 0, &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    files.truncate(MAX_FILES);

    let mut hash_input = String::new();
    for f in &files {
        hash_input.push_str(&format!(
            "{}|{}|{}|{}\n",
            f.relative_path,
            f.size_bytes,
            f.mtime,
            f.content_hash.as_deref().unwrap_or("-")
        ));
    }
    let inventory_hash = extractor::sha256_hex(hash_input.as_bytes());

    Ok(ScanResult { files, inventory_hash })
}

fn walk(
    root: &Path,
    current: &Path,
    depth: usize,
    out: &mut Vec<ProjectFileEntry>,
) -> Result<(), AppError> {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return Ok(());
    }
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()), // サブディレクトリの読み取り失敗はスキップして続行
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return Ok(());
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue; // 隠しファイル・隠しディレクトリ
        }
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_symlink() {
            continue; // ループ・案件外脱出の防止
        }
        let path = entry.path();
        if file_type.is_dir() {
            if IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, depth + 1, out)?;
            continue;
        }
        if depth == 0 && name == CONTEXT_FILE_NAME {
            continue; // 自己参照ループ防止（スペック§4）
        }
        if let Some(file_entry) = build_entry(root, &path) {
            out.push(file_entry);
        }
    }
    Ok(())
}

fn build_entry(root: &Path, path: &Path) -> Option<ProjectFileEntry> {
    let meta = std::fs::metadata(path).ok()?;
    let relative_path = path.strip_prefix(root).ok()?.to_string_lossy().into_owned();
    let mtime: DateTime<Utc> = meta.modified().ok()?.into();
    let content_kind = extractor::content_kind_for(path);

    let (content_hash, extract_status) = if content_kind == "text" {
        if meta.len() > extractor::MAX_HASHABLE_FILE_BYTES {
            (None, "skipped_too_large")
        } else {
            match extractor::extract_text(path) {
                Ok(e) => (Some(e.hash), "ok"),
                Err(_) => (None, "error"),
            }
        }
    } else {
        (None, "unsupported")
    };

    Some(ProjectFileEntry {
        relative_path,
        size_bytes: meta.len() as i64,
        mtime: mtime.to_rfc3339(),
        content_hash,
        content_kind: content_kind.to_string(),
        extract_status: extract_status.to_string(),
    })
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test project_context::scanner`
Expected: 5件 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/project_context/
git commit -m "feat(project-context): ディレクトリ走査とinventory_hash計算を追加"
```

---

### Task 6: project_context/context_file.rs（PIGEON-CONTEXT.md 読み書き）

**Files:**
- Create: `src-tauri/src/project_context/context_file.rs`
- Modify: `src-tauri/src/project_context/mod.rs`（`pub mod context_file;` 追加）

**Interfaces:**
- Produces:
  - `context_file::AUTO_MARKER: &str = "<!-- pigeon:auto -->"`
  - `context_file::MAX_CACHED_CONTEXT_CHARS: usize = 800`
  - `context_file::upsert_auto_section(existing: Option<&str>, project_name: &str, auto_body: &str) -> String`（ユーザー欄不可侵で auto セクションだけ差し替えた新しい全文を返す純粋関数）
  - `context_file::split_at_marker(content: &str) -> (String, Option<String>)`（(ユーザー欄, auto部) に分割。マーカー無しは (全文, None)）
  - `context_file::build_cached_context(full_md: &str, max_chars: usize) -> String`（ユーザー欄優先で切詰め）
  - `context_file::read_context_file(dir: &Path) -> Result<Option<String>, AppError>`
  - `context_file::write_context_file(dir: &Path, content: &str) -> Result<(), AppError>`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upsert_creates_new_file_content() {
        let result = upsert_auto_section(None, "〇〇ホール 春公演", "- 会場: 〇〇ホール");
        assert!(result.starts_with("# 〇〇ホール 春公演"));
        assert!(result.contains(AUTO_MARKER));
        assert!(result.contains("- 会場: 〇〇ホール"));
        // マーカーはユーザー欄の後
        assert!(result.find(AUTO_MARKER).unwrap() > result.find("# 〇〇ホール").unwrap());
    }

    #[test]
    fn test_upsert_preserves_user_section() {
        let existing = format!(
            "# 手書きタイトル\n\n会場担当: 伊藤さん\n\n{}\n古い自動生成内容\n",
            AUTO_MARKER
        );
        let result = upsert_auto_section(Some(&existing), "ignored", "新しい内容");
        assert!(result.contains("# 手書きタイトル"));
        assert!(result.contains("会場担当: 伊藤さん"));
        assert!(result.contains("新しい内容"));
        assert!(!result.contains("古い自動生成内容"));
    }

    #[test]
    fn test_upsert_appends_marker_when_missing() {
        // ユーザーが自作したファイル（マーカー無し）→ 末尾に追加、本文は無傷
        let existing = "# 自作メモ\n大事なこと\n";
        let result = upsert_auto_section(Some(existing), "ignored", "auto内容");
        assert!(result.starts_with("# 自作メモ\n大事なこと\n"));
        assert!(result.contains(AUTO_MARKER));
        assert!(result.contains("auto内容"));
    }

    #[test]
    fn test_upsert_multiple_markers_first_wins() {
        let existing = format!(
            "user部\n{}\n中身1\n{}\n中身2\n",
            AUTO_MARKER, AUTO_MARKER
        );
        let result = upsert_auto_section(Some(&existing), "ignored", "新");
        // 最初のマーカーを正とし、それ以降全体が auto セクションとして置換される
        assert_eq!(result.matches(AUTO_MARKER).count(), 1);
        assert!(!result.contains("中身1"));
        assert!(!result.contains("中身2"));
        assert!(result.contains("新"));
    }

    #[test]
    fn test_build_cached_context_prioritizes_user_section() {
        let user = "ユ".repeat(700);
        let auto = "オ".repeat(700);
        let md = format!("{}\n{}\n{}", user, AUTO_MARKER, auto);
        let cached = build_cached_context(&md, 800);
        assert!(cached.chars().count() <= 800);
        assert!(cached.contains(&"ユ".repeat(700)), "ユーザー欄は全量残る");
        assert!(cached.contains('オ'), "残り枠に auto が入る");
    }

    #[test]
    fn test_read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_context_file(dir.path()).unwrap().is_none());
        write_context_file(dir.path(), "内容").unwrap();
        assert_eq!(read_context_file(dir.path()).unwrap().unwrap(), "内容");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test project_context::context_file`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

```rust
use crate::error::AppError;
use std::path::Path;

pub const AUTO_MARKER: &str = "<!-- pigeon:auto -->";
pub const MAX_CACHED_CONTEXT_CHARS: usize = 800;
const FILE_NAME: &str = "PIGEON-CONTEXT.md";

/// マーカーで (ユーザー欄, auto部) に分割する。マーカー無しは (全文, None)。
/// 複数マーカーは最初のものを正とする（スペック§3 更新規約）。
pub fn split_at_marker(content: &str) -> (String, Option<String>) {
    match content.find(AUTO_MARKER) {
        Some(pos) => {
            let user = content[..pos].to_string();
            let auto = content[pos + AUTO_MARKER.len()..].to_string();
            (user, Some(auto))
        }
        None => (content.to_string(), None),
    }
}

/// auto セクションだけを差し替えた全文を返す。ユーザー欄（マーカーより上）は不可侵。
pub fn upsert_auto_section(
    existing: Option<&str>,
    project_name: &str,
    auto_body: &str,
) -> String {
    let user_section = match existing {
        Some(content) => split_at_marker(content).0,
        None => format!(
            "# {}\n\n（ここから上は自由記入欄です。Pigeon は書き換えません）\n\n",
            project_name
        ),
    };
    let user_trimmed = user_section.trim_end();
    format!("{}\n\n{}\n{}\n", user_trimmed, AUTO_MARKER, auto_body.trim())
}

/// 分類プロンプト注入用の切詰め。ユーザー欄を優先し、残り枠に auto を入れる。
pub fn build_cached_context(full_md: &str, max_chars: usize) -> String {
    let (user, auto) = split_at_marker(full_md);
    let user = user.trim();
    let auto = auto.unwrap_or_default();
    let auto = auto.trim();

    let user_chars: Vec<char> = user.chars().collect();
    if user_chars.len() >= max_chars {
        return user_chars[..max_chars].iter().collect();
    }
    let remaining = max_chars - user_chars.len() - 1; // 改行分
    let auto_part: String = auto.chars().take(remaining).collect();
    if auto_part.is_empty() {
        user.to_string()
    } else {
        format!("{}\n{}", user, auto_part)
    }
}

pub fn read_context_file(dir: &Path) -> Result<Option<String>, AppError> {
    let path = dir.join(FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::DirectoryScan(format!("{}: {}", path.display(), e))),
    }
}

pub fn write_context_file(dir: &Path, content: &str) -> Result<(), AppError> {
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, content)
        .map_err(|e| AppError::DirectoryScan(format!("{}: {}", path.display(), e)))
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test project_context::context_file`
Expected: 6件 PASS。`cargo clippy -- -D warnings` も確認（PR2 の締め）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/project_context/
git commit -m "feat(project-context): PIGEON-CONTEXT.mdのマーカー処理と読み書きを追加"
```

---

### Task 7: クラウド送信ルール（db/cloud_rules.rs + cloud_policy.rs）

**Files:**
- Create: `src-tauri/src/db/cloud_rules.rs`
- Create: `src-tauri/src/project_context/cloud_policy.rs`
- Modify: `src-tauri/src/db/mod.rs` / `src-tauri/src/project_context/mod.rs`

**Interfaces:**
- Consumes: `CloudRule`（Task 2）
- Produces:
  - `db::cloud_rules::set_rule(conn, directory_id, scope, relative_path, allow) -> Result<(), AppError>`（UPSERT）
  - `db::cloud_rules::delete_rule(conn, directory_id, scope, relative_path) -> Result<(), AppError>`
  - `db::cloud_rules::list_rules(conn, directory_id) -> Result<Vec<CloudRule>, AppError>`
  - `cloud_policy::is_cloud_allowed(rules: &[CloudRule], relative_path: &str) -> bool`（**不変条件: マッチ無し→false。最長 relative_path 優先。同長は file スコープ優先**）

- [ ] **Step 1: 失敗するテストを書く**

`cloud_policy.rs`（判定ロジック。危険側テストを厚く）:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::directory::CloudRule;

    fn rule(scope: &str, path: &str, allow: bool) -> CloudRule {
        CloudRule {
            id: format!("r-{}-{}", scope, path),
            directory_id: "d1".to_string(),
            scope: scope.to_string(),
            relative_path: path.to_string(),
            allow,
        }
    }

    #[test]
    fn test_no_rules_means_denied() {
        assert!(!is_cloud_allowed(&[], "図面/平面図.pdf"));
    }

    #[test]
    fn test_directory_allow_covers_children() {
        let rules = vec![rule("directory", "図面", true)];
        assert!(is_cloud_allowed(&rules, "図面/平面図.pdf"));
        assert!(is_cloud_allowed(&rules, "図面/sub/詳細.pdf"));
        assert!(!is_cloud_allowed(&rules, "契約/見積.pdf"), "許可外はfalse");
        assert!(!is_cloud_allowed(&rules, "図面外.txt"), "前方一致の誤爆をしない");
    }

    #[test]
    fn test_root_directory_rule_covers_all() {
        let rules = vec![rule("directory", "", true)];
        assert!(is_cloud_allowed(&rules, "anything.txt"));
        assert!(is_cloud_allowed(&rules, "a/b/c.txt"));
    }

    #[test]
    fn test_explicit_file_deny_beats_parent_allow() {
        let rules = vec![
            rule("directory", "", true),
            rule("file", "予算メモ.md", false),
        ];
        assert!(is_cloud_allowed(&rules, "他.txt"));
        assert!(!is_cloud_allowed(&rules, "予算メモ.md"), "明示除外が親許可に勝つ");
    }

    #[test]
    fn test_longest_match_wins() {
        let rules = vec![
            rule("directory", "図面", true),
            rule("directory", "図面/社外秘", false),
        ];
        assert!(is_cloud_allowed(&rules, "図面/平面図.pdf"));
        assert!(!is_cloud_allowed(&rules, "図面/社外秘/原価.txt"));
    }

    #[test]
    fn test_file_scope_requires_exact_match() {
        let rules = vec![rule("file", "香盤表.md", true)];
        assert!(is_cloud_allowed(&rules, "香盤表.md"));
        assert!(!is_cloud_allowed(&rules, "香盤表.md.bak"));
        assert!(!is_cloud_allowed(&rules, "sub/香盤表.md"));
    }

    #[test]
    fn test_file_scope_beats_directory_scope_at_same_length() {
        let rules = vec![
            rule("directory", "a/b.txt", true), // 不正気味なルールでも
            rule("file", "a/b.txt", false),     // fileスコープが勝つ
        ];
        assert!(!is_cloud_allowed(&rules, "a/b.txt"));
    }
}
```

`db/cloud_rules.rs` のテスト:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn setup_dir(conn: &Connection) -> String {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(conn, "p1", "/tmp/x").unwrap().id
    }

    #[test]
    fn test_set_rule_upserts() {
        let conn = setup_db();
        let dir_id = setup_dir(&conn);

        set_rule(&conn, &dir_id, "directory", "図面", true).unwrap();
        set_rule(&conn, &dir_id, "directory", "図面", false).unwrap(); // 同キーは上書き

        let rules = list_rules(&conn, &dir_id).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].allow);
    }

    #[test]
    fn test_delete_rule() {
        let conn = setup_db();
        let dir_id = setup_dir(&conn);
        set_rule(&conn, &dir_id, "file", "a.txt", true).unwrap();
        delete_rule(&conn, &dir_id, "file", "a.txt").unwrap();
        assert!(list_rules(&conn, &dir_id).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test cloud`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

`project_context/cloud_policy.rs`:

```rust
use crate::models::directory::CloudRule;

/// クラウド送信可否の判定（スペック§5 不変条件）:
/// - マッチするルールが無ければ常に false（危険側に倒れない）
/// - 最長 relative_path のルールが勝つ。同長なら file スコープが勝つ
/// - directory スコープは prefix マッチ（'' は全体）、file スコープは完全一致
pub fn is_cloud_allowed(rules: &[CloudRule], relative_path: &str) -> bool {
    let mut best: Option<&CloudRule> = None;
    for rule in rules {
        let matches = match rule.scope.as_str() {
            "file" => rule.relative_path == relative_path,
            "directory" => {
                rule.relative_path.is_empty()
                    || relative_path == rule.relative_path
                    || relative_path.starts_with(&format!("{}/", rule.relative_path))
            }
            _ => false,
        };
        if !matches {
            continue;
        }
        best = match best {
            None => Some(rule),
            Some(current) => {
                let longer = rule.relative_path.len() > current.relative_path.len();
                let same_len_file_wins = rule.relative_path.len() == current.relative_path.len()
                    && rule.scope == "file"
                    && current.scope != "file";
                if longer || same_len_file_wins {
                    Some(rule)
                } else {
                    Some(current)
                }
            }
        };
    }
    best.map(|r| r.allow).unwrap_or(false)
}
```

`db/cloud_rules.rs`:

```rust
use crate::error::AppError;
use crate::models::directory::CloudRule;
use rusqlite::{params, Connection};
use uuid::Uuid;

pub fn set_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
    allow: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_cloud_rules (id, directory_id, scope, relative_path, allow)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(directory_id, scope, relative_path) DO UPDATE SET allow = ?5",
        params![Uuid::new_v4().to_string(), directory_id, scope, relative_path, allow],
    )?;
    Ok(())
}

pub fn delete_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM project_cloud_rules
         WHERE directory_id = ?1 AND scope = ?2 AND relative_path = ?3",
        params![directory_id, scope, relative_path],
    )?;
    Ok(())
}

pub fn list_rules(conn: &Connection, directory_id: &str) -> Result<Vec<CloudRule>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, directory_id, scope, relative_path, allow
         FROM project_cloud_rules WHERE directory_id = ?1 ORDER BY relative_path",
    )?;
    let rules = stmt
        .query_map(params![directory_id], |row| {
            Ok(CloudRule {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                scope: row.get(2)?,
                relative_path: row.get(3)?,
                allow: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rules)
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test cloud`
Expected: 9件 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/cloud_rules.rs src-tauri/src/db/mod.rs src-tauri/src/project_context/
git commit -m "feat(project-context): クラウド送信許可ルールと最長マッチ判定を追加"
```

---

### Task 8: db/project_contexts.rs（コンテキストキャッシュ）

**Files:**
- Create: `src-tauri/src/db/project_contexts.rs`
- Modify: `src-tauri/src/db/mod.rs`

**Interfaces:**
- Consumes: `ProjectContext`（Task 2）
- Produces:
  - `db::project_contexts::get_context(conn, project_id) -> Result<Option<ProjectContext>, AppError>`
  - `db::project_contexts::upsert_generated(conn, project_id, cached_context, context_hash, inventory_hash) -> Result<(), AppError>`（`generated_at = CURRENT_TIMESTAMP`）
  - `db::project_contexts::update_cache_only(conn, project_id, cached_context, context_hash) -> Result<(), AppError>`（自己修復用。inventory_hash と generated_at は触らない）
  - `db::project_contexts::set_allow_cloud_context(conn, project_id, allow) -> Result<(), AppError>`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_get_context_none_initially() {
        let conn = setup_db();
        create_project(&conn);
        assert!(get_context(&conn, "p1").unwrap().is_none());
    }

    #[test]
    fn test_upsert_generated_and_get() {
        let conn = setup_db();
        create_project(&conn);

        upsert_generated(&conn, "p1", "コンテキスト", "chash1", "ihash1").unwrap();
        let ctx = get_context(&conn, "p1").unwrap().unwrap();
        assert_eq!(ctx.cached_context.as_deref(), Some("コンテキスト"));
        assert_eq!(ctx.context_hash.as_deref(), Some("chash1"));
        assert_eq!(ctx.inventory_hash.as_deref(), Some("ihash1"));
        assert!(!ctx.allow_cloud_context, "デフォルトは送信不許可");
        assert!(ctx.generated_at.is_some());

        // 2回目は上書き
        upsert_generated(&conn, "p1", "更新後", "chash2", "ihash2").unwrap();
        let ctx = get_context(&conn, "p1").unwrap().unwrap();
        assert_eq!(ctx.cached_context.as_deref(), Some("更新後"));
    }

    #[test]
    fn test_set_allow_cloud_context_survives_upsert() {
        let conn = setup_db();
        create_project(&conn);
        upsert_generated(&conn, "p1", "c", "h", "i").unwrap();
        set_allow_cloud_context(&conn, "p1", true).unwrap();
        // 再生成してもユーザーの許可設定は消えない
        upsert_generated(&conn, "p1", "c2", "h2", "i2").unwrap();
        assert!(get_context(&conn, "p1").unwrap().unwrap().allow_cloud_context);
    }

    #[test]
    fn test_update_cache_only_keeps_inventory_hash() {
        let conn = setup_db();
        create_project(&conn);
        upsert_generated(&conn, "p1", "c", "h", "ihash").unwrap();
        update_cache_only(&conn, "p1", "手編集後", "h2").unwrap();
        let ctx = get_context(&conn, "p1").unwrap().unwrap();
        assert_eq!(ctx.cached_context.as_deref(), Some("手編集後"));
        assert_eq!(ctx.inventory_hash.as_deref(), Some("ihash"), "inventory_hashは不変");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::project_contexts`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

```rust
use crate::error::AppError;
use crate::models::directory::ProjectContext;
use rusqlite::{params, Connection, OptionalExtension};

pub fn get_context(conn: &Connection, project_id: &str) -> Result<Option<ProjectContext>, AppError> {
    conn.query_row(
        "SELECT project_id, cached_context, context_hash, inventory_hash,
                allow_cloud_context, generated_at
         FROM project_contexts WHERE project_id = ?1",
        params![project_id],
        |row| {
            Ok(ProjectContext {
                project_id: row.get(0)?,
                cached_context: row.get(1)?,
                context_hash: row.get(2)?,
                inventory_hash: row.get(3)?,
                allow_cloud_context: row.get(4)?,
                generated_at: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(AppError::Database)
}

pub fn upsert_generated(
    conn: &Connection,
    project_id: &str,
    cached_context: &str,
    context_hash: &str,
    inventory_hash: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_contexts
            (project_id, cached_context, context_hash, inventory_hash, generated_at)
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            cached_context = ?2, context_hash = ?3, inventory_hash = ?4,
            generated_at = CURRENT_TIMESTAMP",
        params![project_id, cached_context, context_hash, inventory_hash],
    )?;
    Ok(())
}

/// 自己修復用: PIGEON-CONTEXT.md の外部編集を検知したときにキャッシュだけ更新する。
pub fn update_cache_only(
    conn: &Connection,
    project_id: &str,
    cached_context: &str,
    context_hash: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_contexts (project_id, cached_context, context_hash)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(project_id) DO UPDATE SET cached_context = ?2, context_hash = ?3",
        params![project_id, cached_context, context_hash],
    )?;
    Ok(())
}

pub fn set_allow_cloud_context(
    conn: &Connection,
    project_id: &str,
    allow: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_contexts (project_id, allow_cloud_context)
         VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET allow_cloud_context = ?2",
        params![project_id, allow],
    )?;
    Ok(())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test db::project_contexts`
Expected: 4件 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/project_contexts.rs src-tauri/src/db/mod.rs
git commit -m "feat(db): 案件コンテキストキャッシュのCRUDを追加"
```

---

### Task 9: ダイジェスト生成（TextGenerator trait + digest.rs）

**Files:**
- Modify: `src-tauri/src/classifier/mod.rs`（`TextGenerator` trait 追加）
- Modify: `src-tauri/src/classifier/ollama.rs`（chat 呼び出しの共通化 + `TextGenerator` 実装）
- Create: `src-tauri/src/project_context/digest.rs`
- Modify: `src-tauri/src/project_context/mod.rs`

**Interfaces:**
- Produces:
  - `classifier::TextGenerator` trait: `async fn generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError>`（`OllamaClassifier` が実装。将来 ClaudeClassifier も実装する）
  - `digest::build_digest_input(project_name: &str, files: &[ProjectFileEntry], texts: &[(String, String)]) -> String`（(相対パス, 抽出テキスト) のリスト。呼び出し側が送信可否と100KB上限を適用済みであること）
  - `digest::generate_digest(generator: &dyn TextGenerator, input: &str) -> Result<String, AppError>`
  - `digest::DIGEST_SYSTEM_PROMPT: &str`

- [ ] **Step 1: 失敗するテストを書く**

`digest.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::TextGenerator;
    use crate::error::AppError;
    use crate::models::directory::ProjectFileEntry;
    use async_trait::async_trait;

    struct MockGenerator {
        response: String,
    }

    #[async_trait]
    impl TextGenerator for MockGenerator {
        async fn generate_text(&self, _system: &str, _user: &str) -> Result<String, AppError> {
            Ok(self.response.clone())
        }
    }

    fn entry(path: &str) -> ProjectFileEntry {
        ProjectFileEntry {
            relative_path: path.to_string(),
            size_bytes: 10,
            mtime: "2026-07-09T00:00:00Z".to_string(),
            content_hash: None,
            content_kind: "text".to_string(),
            extract_status: "ok".to_string(),
        }
    }

    #[test]
    fn test_build_digest_input_contains_files_and_texts() {
        let files = vec![entry("図面/平面図.pdf"), entry("香盤表.md")];
        let texts = vec![("香盤表.md".to_string(), "第1幕 くるみ割り".to_string())];
        let input = build_digest_input("〇〇ホール 春公演", &files, &texts);

        assert!(input.contains("〇〇ホール 春公演"));
        assert!(input.contains("図面/平面図.pdf"));
        assert!(input.contains("第1幕 くるみ割り"));
    }

    #[tokio::test]
    async fn test_generate_digest_returns_llm_output() {
        let generator = MockGenerator {
            response: "- 公演: くるみ割り人形\n- 会場: 〇〇ホール".to_string(),
        };
        let digest = generate_digest(&generator, "input").await.unwrap();
        assert!(digest.contains("くるみ割り人形"));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test project_context::digest`
Expected: コンパイルエラー（`TextGenerator` 未定義）

- [ ] **Step 3: 実装**

`classifier/mod.rs` に追加:

```rust
/// 汎用テキスト生成（ダイジェスト生成等に使用）。LlmClassifier と同じプロバイダが実装する。
#[async_trait]
pub trait TextGenerator: Send + Sync {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError>;
}
```

`classifier/ollama.rs`: `classify` 内の chat 呼び出しを private メソッドに抽出して共用する:

```rust
impl OllamaClassifier {
    /// /api/chat を呼び、応答テキストを返す（classify と TextGenerator の共通部）。
    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let request_body = OllamaChatRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                OllamaMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            stream: false,
        };

        let url = format!("{}/api/chat", self.endpoint);
        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::OllamaConnection(format!(
                "Ollama returned status {}",
                response.status()
            )));
        }

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| AppError::InvalidLlmResponse(e.to_string()))?;
        Ok(chat_response.message.content)
    }
}

#[async_trait]
impl crate::classifier::TextGenerator for OllamaClassifier {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError> {
        self.chat(system_prompt, user_prompt).await
    }
}
```

`classify` の本体は `let content = self.chat(prompt::SYSTEM_PROMPT, &user_prompt).await?;` に置き換え（挙動不変のリファクタ。既存テストが緑のままであることを確認）。

`project_context/digest.rs`:

```rust
use crate::classifier::TextGenerator;
use crate::error::AppError;
use crate::models::directory::ProjectFileEntry;

pub const DIGEST_SYSTEM_PROMPT: &str = "\
あなたは舞台制作の案件アシスタントです。案件フォルダのファイル一覧とテキスト資料から、
この案件の要約を Markdown の箇条書きで出力してください。

出力形式（この形式のみ、前置き・後置きなし）:
- 公演: <公演名・演目>
- 会場: <会場名とキーワード>
- 関係する組織・人: <資料から読み取れる関係先>
- キーワード: <メール分類の手がかりになる語>
- 主なファイル: <代表的なファイル名 5件まで>

読み取れない項目は行ごと省略する。推測で埋めない。全体で400字以内。";

pub fn build_digest_input(
    project_name: &str,
    files: &[ProjectFileEntry],
    texts: &[(String, String)],
) -> String {
    let mut input = format!("## 案件名\n{}\n\n## ファイル一覧\n", project_name);
    for f in files {
        input.push_str(&format!("- {}\n", f.relative_path));
    }
    if !texts.is_empty() {
        input.push_str("\n## テキスト資料の内容\n");
        for (path, text) in texts {
            input.push_str(&format!("### {}\n{}\n\n", path, text));
        }
    }
    input
}

pub async fn generate_digest(
    generator: &dyn TextGenerator,
    input: &str,
) -> Result<String, AppError> {
    generator.generate_text(DIGEST_SYSTEM_PROMPT, input).await
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test project_context::digest && cargo test classifier`
Expected: digest 2件 PASS + 既存 classifier テスト全 PASS（リファクタで壊れていないこと）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/classifier/ src-tauri/src/project_context/
git commit -m "feat(classifier): TextGenerator traitとダイジェスト生成を追加"
```

---

### Task 10: 再スキャンオーケストレータ（project_context/mod.rs）

**Files:**
- Modify: `src-tauri/src/project_context/mod.rs`

**Interfaces:**
- Consumes: Task 2–9 の全 API
- Produces:
  - `project_context::rescan_project(db: &std::sync::Mutex<rusqlite::Connection>, generator: &dyn TextGenerator, project_id: &str, cloud: bool) -> Result<RescanOutcome, AppError>`（async）
  - `#[derive(Serialize)] pub struct RescanOutcome { pub status: String, pub regenerated: bool, pub file_count: usize }`
  - `cloud: bool` = ダイジェスト生成に使う LLM がクラウドかどうか。**現状は Ollama のみなので呼び出し側は常に `false`**。Claude 対応時にプロバイダ設定から渡す

- [ ] **Step 1: 失敗するテストを書く**

`project_context/mod.rs` の tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::TextGenerator;
    use crate::error::AppError;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct MockGenerator;

    #[async_trait]
    impl TextGenerator for MockGenerator {
        async fn generate_text(&self, _s: &str, _u: &str) -> Result<String, AppError> {
            Ok("- 会場: 〇〇ホール".to_string())
        }
    }

    struct FailGenerator;

    #[async_trait]
    impl TextGenerator for FailGenerator {
        async fn generate_text(&self, _s: &str, _u: &str) -> Result<String, AppError> {
            Err(AppError::OllamaConnection("down".to_string()))
        }
    }

    fn setup(dir_path: &str) -> Mutex<rusqlite::Connection> {
        let conn = crate::test_helpers::setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', '春公演')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(&conn, "p1", dir_path).unwrap();
        Mutex::new(conn)
    }

    #[tokio::test]
    async fn test_rescan_generates_context_file_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("香盤表.md"), "第1幕").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        let outcome = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert_eq!(outcome.status, "ok");
        assert!(outcome.regenerated);
        assert_eq!(outcome.file_count, 1);

        // PIGEON-CONTEXT.md が生成されている
        let md = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(md.contains("〇〇ホール"));
        assert!(md.contains(context_file::AUTO_MARKER));

        // キャッシュも更新されている
        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1").unwrap().unwrap();
        assert!(ctx.cached_context.unwrap().contains("〇〇ホール"));
        assert!(ctx.inventory_hash.is_some());
    }

    #[tokio::test]
    async fn test_rescan_unchanged_skips_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        let first = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert!(first.regenerated);
        let second = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert!(!second.regenerated, "構成が同じならLLMを呼ばない");
    }

    #[tokio::test]
    async fn test_rescan_preserves_user_section_on_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        // ユーザーが自由記入欄を編集
        let md_path = dir.path().join("PIGEON-CONTEXT.md");
        let md = std::fs::read_to_string(&md_path).unwrap();
        let edited = md.replace("# 春公演", "# 春公演\n会場担当: 伊藤さん");
        std::fs::write(&md_path, edited).unwrap();

        // ファイル追加 → 再生成
        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        let md = std::fs::read_to_string(&md_path).unwrap();
        assert!(md.contains("会場担当: 伊藤さん"), "ユーザー欄は不可侵");
    }

    #[tokio::test]
    async fn test_rescan_missing_directory_sets_status_and_keeps_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        // ディレクトリ消失（外付けHDD未接続を模擬）
        drop(dir);
        let outcome = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert_eq!(outcome.status, "missing");

        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1").unwrap().unwrap();
        assert!(ctx.cached_context.is_some(), "キャッシュは消さず分類に使い続ける");
    }

    #[tokio::test]
    async fn test_rescan_llm_failure_keeps_previous_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        let outcome = rescan_project(&db, &FailGenerator, "p1", false).await.unwrap();
        assert_eq!(outcome.status, "ok");
        assert!(!outcome.regenerated, "LLM失敗時は再生成失敗として扱う");

        let md = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(md.contains("〇〇ホール"), "前回のautoセクションを維持（劣化しない）");
    }

    #[tokio::test]
    async fn test_rescan_cloud_mode_excludes_unallowed_files_from_input() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct SpyGenerator {
            saw_secret: std::sync::Arc<AtomicBool>,
        }
        #[async_trait]
        impl TextGenerator for SpyGenerator {
            async fn generate_text(&self, _s: &str, user: &str) -> Result<String, AppError> {
                if user.contains("秘密") || user.contains("secret.txt") {
                    self.saw_secret.store(true, Ordering::SeqCst);
                }
                Ok("- 要約".to_string())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("public.txt"), "公開資料").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "秘密").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        {
            let conn = db.lock().unwrap();
            let d = crate::db::directories::get_directory_by_project(&conn, "p1")
                .unwrap()
                .unwrap();
            crate::db::cloud_rules::set_rule(&conn, &d.id, "file", "public.txt", true).unwrap();
        }

        let saw_secret = std::sync::Arc::new(AtomicBool::new(false));
        let spy = SpyGenerator { saw_secret: saw_secret.clone() };
        rescan_project(&db, &spy, "p1", true).await.unwrap();

        assert!(
            !saw_secret.load(Ordering::SeqCst),
            "cloud=true では未許可ファイルは名前も内容もLLMに渡さない（スペック§5不変条件1）"
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test project_context::tests`
Expected: コンパイルエラー（`rescan_project` 未定義）

- [ ] **Step 3: 実装**

`project_context/mod.rs`:

```rust
pub mod cloud_policy;
pub mod context_file;
pub mod digest;
pub mod extractor;
pub mod scanner;

use crate::classifier::TextGenerator;
use crate::db::{cloud_rules, directories, project_contexts, project_files, projects};
use crate::error::AppError;
use crate::models::directory::ProjectFileEntry;
use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct RescanOutcome {
    pub status: String,      // 'ok' | 'missing' | 'inaccessible' | 'error' | 'unlinked'
    pub regenerated: bool,   // auto セクションを再生成したか
    pub file_count: usize,
}

/// 案件ディレクトリの再スキャン一式。
/// ロックは「DBスナップショット取得」「結果書き込み」の2回だけ短く取り、
/// ファイルI/OとLLM呼び出しはロック外で行う（classify_commands と同じ様式）。
pub async fn rescan_project(
    db: &Mutex<Connection>,
    generator: &dyn TextGenerator,
    project_id: &str,
    cloud: bool,
) -> Result<RescanOutcome, AppError> {
    // --- 1. スナップショット取得（ロック内） ---
    let (dir, project_name, prev_inventory_hash, rules) = {
        let conn = db.lock().map_err(AppError::lock_err)?;
        let dir = match directories::get_directory_by_project(&conn, project_id)? {
            Some(d) => d,
            None => {
                return Ok(RescanOutcome {
                    status: "unlinked".to_string(),
                    regenerated: false,
                    file_count: 0,
                })
            }
        };
        let project = projects::get_project(&conn, project_id)?;
        let prev = project_contexts::get_context(&conn, project_id)?
            .and_then(|c| c.inventory_hash);
        let rules = cloud_rules::list_rules(&conn, &dir.id)?;
        (dir, project.name, prev, rules)
    };

    let root = Path::new(&dir.path);

    // --- 2. スキャン（ロック外） ---
    let scan = match scanner::scan_directory(root) {
        Ok(s) => s,
        Err(AppError::DirectoryScan(msg)) => {
            // "missing" / "inaccessible" / "error" を文言から判別（scanner が付与）
            let status = if msg.contains("[missing]") {
                "missing"
            } else if msg.contains("[inaccessible]") {
                "inaccessible"
            } else {
                "error"
            };
            let conn = db.lock().map_err(AppError::lock_err)?;
            directories::set_status(&conn, &dir.id, status)?;
            // キャッシュは消さない（スペック§8: 分類に使い続ける）
            return Ok(RescanOutcome {
                status: status.to_string(),
                regenerated: false,
                file_count: 0,
            });
        }
        Err(e) => return Err(e),
    };

    // --- 3. インベントリ書き込み（ロック内） ---
    {
        let mut conn = db.lock().map_err(AppError::lock_err)?;
        project_files::replace_inventory(&mut conn, &dir.id, &scan.files)?;
        directories::set_status(&conn, &dir.id, "ok")?;
        directories::touch_scanned(&conn, &dir.id)?;
    }

    // --- 4. 構成不変なら自己修復のみ（md外部編集の取り込み） ---
    if prev_inventory_hash.as_deref() == Some(scan.inventory_hash.as_str()) {
        if let Some(md) = context_file::read_context_file(root)? {
            let cached =
                context_file::build_cached_context(&md, context_file::MAX_CACHED_CONTEXT_CHARS);
            let hash = extractor::sha256_hex(md.as_bytes());
            let conn = db.lock().map_err(AppError::lock_err)?;
            project_contexts::update_cache_only(&conn, project_id, &cached, &hash)?;
        }
        return Ok(RescanOutcome {
            status: "ok".to_string(),
            regenerated: false,
            file_count: scan.files.len(),
        });
    }

    // --- 5. ダイジェスト入力の組み立て（送信可否 + 100KB 上限を適用） ---
    let visible_files: Vec<ProjectFileEntry> = if cloud {
        // スペック§5不変条件1: 未許可ファイルは名前も含めない
        scan.files
            .iter()
            .filter(|f| cloud_policy::is_cloud_allowed(&rules, &f.relative_path))
            .cloned()
            .collect()
    } else {
        scan.files.clone()
    };

    let mut texts: Vec<(String, String)> = Vec::new();
    let mut budget = extractor::MAX_EXTRACT_BYTES_PER_PROJECT;
    for f in &visible_files {
        if f.content_kind != "text" || f.extract_status != "ok" || budget == 0 {
            continue;
        }
        if let Ok(extracted) = extractor::extract_text(&root.join(&f.relative_path)) {
            let take = extracted.text.len().min(budget);
            let mut text = extracted.text;
            text.truncate(take);
            budget -= take;
            texts.push((f.relative_path.clone(), text));
        }
    }

    // 入力が空（cloud で許可ゼロ等）なら生成をスキップして前回を維持（スペック§3）
    if visible_files.is_empty() {
        return Ok(RescanOutcome {
            status: "ok".to_string(),
            regenerated: false,
            file_count: scan.files.len(),
        });
    }

    // --- 6. LLM でダイジェスト生成（ロック外）。失敗時は前回を維持 ---
    let input = digest::build_digest_input(&project_name, &visible_files, &texts);
    let digest_body = match digest::generate_digest(generator, &input).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[warn] digest generation failed for {}: {}", project_id, e);
            return Ok(RescanOutcome {
                status: "ok".to_string(),
                regenerated: false,
                file_count: scan.files.len(),
            });
        }
    };

    // --- 7. PIGEON-CONTEXT.md 更新（ユーザー欄不可侵） ---
    let existing = context_file::read_context_file(root)?;
    let auto_body = format!(
        "## 案件コンテキスト（自動生成 {}）\n\n{}",
        Utc::now().format("%Y-%m-%d"),
        digest_body.trim()
    );
    let new_md =
        context_file::upsert_auto_section(existing.as_deref(), &project_name, &auto_body);
    context_file::write_context_file(root, &new_md)?;

    // --- 8. キャッシュ更新（ロック内） ---
    let cached =
        context_file::build_cached_context(&new_md, context_file::MAX_CACHED_CONTEXT_CHARS);
    let context_hash = extractor::sha256_hex(new_md.as_bytes());
    {
        let conn = db.lock().map_err(AppError::lock_err)?;
        project_contexts::upsert_generated(
            &conn,
            project_id,
            &cached,
            &context_hash,
            &scan.inventory_hash,
        )?;
    }

    Ok(RescanOutcome {
        status: "ok".to_string(),
        regenerated: true,
        file_count: scan.files.len(),
    })
}
```

注意: `scanner::scan_directory` のエラーメッセージに `[missing]` 等の判別子を含める実装（Task 5 の `classify_io_error` を使った `format!("{} [{}]", e, ...)`）が前提。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test project_context`
Expected: 全 PASS（Task 4–6 のテスト含む）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/project_context/mod.rs
git commit -m "feat(project-context): 再スキャンオーケストレータを追加"
```

---

### Task 11: 分類プロンプトへのコンテキスト注入

**Files:**
- Modify: `src-tauri/src/models/classifier.rs`（`ProjectSummary` に `context` 追加）
- Modify: `src-tauri/src/db/projects.rs`（`build_project_summaries` に `for_cloud` 引数）
- Modify: `src-tauri/src/classifier/prompt.rs`（Context 行の注入）
- Modify: `src-tauri/src/commands/classify_commands.rs`（呼び出し2箇所に `false` を渡す）

**Interfaces:**
- Produces:
  - `ProjectSummary.context: Option<String>`（800字上限で切詰済みのコンテキスト）
  - `build_project_summaries(conn, account_id, for_cloud: bool)`（**`for_cloud=true` のとき `allow_cloud_context=false` の案件は `context=None`**。現状の呼び出しはすべて `false`（Ollama）。Claude 対応時にプロバイダ設定で切り替える）

- [ ] **Step 1: 失敗するテストを書く**

`prompt.rs` の tests に追加:

```rust
#[test]
fn test_build_user_prompt_includes_project_context() {
    let mail = make_mail();
    let projects = vec![ProjectSummary {
        id: "p1".to_string(),
        name: "春公演".to_string(),
        description: None,
        recent_subjects: vec![],
        context: Some("会場: 〇〇ホール\n重量制限に注意".to_string()),
    }];
    let prompt = build_user_prompt(&mail, &projects, &[]);
    assert!(prompt.contains("Context:"));
    assert!(prompt.contains("会場: 〇〇ホール"));
}

#[test]
fn test_build_user_prompt_no_context_line_when_none() {
    let mail = make_mail();
    let projects = vec![ProjectSummary {
        id: "p1".to_string(),
        name: "春公演".to_string(),
        description: None,
        recent_subjects: vec![],
        context: None,
    }];
    let prompt = build_user_prompt(&mail, &projects, &[]);
    assert!(!prompt.contains("Context:"));
}
```

`db/projects.rs` の tests に追加:

```rust
#[test]
fn test_build_project_summaries_includes_cached_context() {
    let conn = setup_db();
    let p = insert_project(&conn, &sample_create_req("acc1")).unwrap();
    crate::db::project_contexts::upsert_generated(&conn, &p.id, "会場: 〇〇ホール", "h", "i")
        .unwrap();

    let summaries = build_project_summaries(&conn, "acc1", false).unwrap();
    assert_eq!(summaries[0].context.as_deref(), Some("会場: 〇〇ホール"));
}

#[test]
fn test_build_project_summaries_cloud_excludes_unallowed_context() {
    let conn = setup_db();
    let p = insert_project(&conn, &sample_create_req("acc1")).unwrap();
    crate::db::project_contexts::upsert_generated(&conn, &p.id, "秘密のコンテキスト", "h", "i")
        .unwrap();
    // allow_cloud_context はデフォルト false のまま

    let summaries = build_project_summaries(&conn, "acc1", true).unwrap();
    assert!(
        summaries[0].context.is_none(),
        "スペック§5不変条件2: 未許可案件のコンテキストはクラウドに注入しない"
    );

    // 許可すると注入される
    crate::db::project_contexts::set_allow_cloud_context(&conn, &p.id, true).unwrap();
    let summaries = build_project_summaries(&conn, "acc1", true).unwrap();
    assert!(summaries[0].context.is_some());
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test build_project_summaries && cargo test prompt`
Expected: コンパイルエラー（`context` フィールド未定義）

- [ ] **Step 3: 実装**

`models/classifier.rs` の `ProjectSummary` に追加:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub recent_subjects: Vec<String>,
    pub context: Option<String>,
}
```

コンパイルエラーになる既存の `ProjectSummary { ... }` 構築箇所をすべて `context: None` で修正する（`rg "ProjectSummary \{" src-tauri/src` で検索。`db/projects.rs` の `build_project_summaries`、`classifier/prompt.rs` のテストヘルパ `make_project` 等）。

`db/projects.rs` の `build_project_summaries` を変更:

```rust
/// Build ProjectSummary list for LLM classification context.
///
/// `for_cloud=true` のときは allow_cloud_context が付いた案件のみ context を注入する
/// （スペック§5不変条件2）。Ollama（ローカル）は false で全案件注入。
pub fn build_project_summaries(
    conn: &Connection,
    account_id: &str,
    for_cloud: bool,
) -> Result<Vec<ProjectSummary>, AppError> {
    let projs = list_projects(conn, account_id)?;
    let mut summaries = Vec::with_capacity(projs.len());
    for p in projs {
        let recent_subjects = assignments::get_recent_subjects(conn, &p.id, 5).unwrap_or_default();
        let context = crate::db::project_contexts::get_context(conn, &p.id)?
            .filter(|c| !for_cloud || c.allow_cloud_context)
            .and_then(|c| c.cached_context)
            .map(|c| c.chars().take(800).collect::<String>());
        summaries.push(ProjectSummary {
            id: p.id,
            name: p.name,
            description: p.description,
            recent_subjects,
            context,
        });
    }
    Ok(summaries)
}
```

`classifier/prompt.rs` の `build_user_prompt` の recent_subjects ブロックの直後に追加:

```rust
            if let Some(context) = project.context.as_deref() {
                prompt.push_str(&format!(
                    "  Context: {}\n",
                    context.replace('\n', " / ")
                ));
            }
```

`commands/classify_commands.rs` の `build_project_summaries` 呼び出し2箇所（`classify_mail` / `classify_unassigned`）に第3引数 `false` を追加（Ollama 固定のため）。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS（既存テストの `ProjectSummary` 構築修正漏れがないこと）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/classifier.rs src-tauri/src/db/projects.rs \
        src-tauri/src/classifier/prompt.rs src-tauri/src/commands/classify_commands.rs
git commit -m "feat(classifier): 案件コンテキストを分類プロンプトに注入"
```

---

### Task 12: Tauri commands + 起動時スキャン

**Files:**
- Create: `src-tauri/src/commands/directory_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`（`pub mod directory_commands;`）
- Modify: `src-tauri/src/lib.rs`（コマンド登録 + 起動時バックグラウンドスキャン）

**Interfaces:**
- Produces（UI プランが consume する API。名前・型はここが正）:
  - `link_project_directory(project_id: String, path: String) -> ProjectDirectory`
  - `unlink_project_directory(project_id: String) -> ()`
  - `get_project_directory(project_id: String) -> Option<ProjectDirectory>`
  - `rescan_project_directory(project_id: String) -> RescanOutcome`（async。Ollama を settings から構築）
  - `list_project_files(directory_id: String) -> Vec<ProjectFile>`
  - `set_cloud_rule(directory_id: String, scope: String, relative_path: String, allow: Option<bool>) -> ()`（`allow=None` はルール削除）
  - `get_cloud_rules(directory_id: String) -> Vec<CloudRule>`
  - `set_allow_cloud_context(project_id: String, allow: bool) -> ()`
  - `get_project_context(project_id: String) -> Option<ProjectContext>`

- [ ] **Step 1: 失敗するテストを書く**

`directory_commands.rs`（コマンド関数はロジックを db/project_context 層に委譲するため、テストは委譲ロジックの組み合わせを検証）:

```rust
#[cfg(test)]
mod tests {
    use crate::db::{cloud_rules, directories};
    use crate::test_helpers::setup_db;

    #[test]
    fn test_set_cloud_rule_none_deletes() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
        let dir = directories::link_directory(&conn, "p1", "/tmp/x").unwrap();

        super::apply_cloud_rule(&conn, &dir.id, "file", "a.txt", Some(true)).unwrap();
        assert_eq!(cloud_rules::list_rules(&conn, &dir.id).unwrap().len(), 1);

        super::apply_cloud_rule(&conn, &dir.id, "file", "a.txt", None).unwrap();
        assert!(cloud_rules::list_rules(&conn, &dir.id).unwrap().is_empty());
    }

    #[test]
    fn test_link_validates_path_is_absolute() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
        let result = super::validate_and_link(&conn, "p1", "relative/path");
        assert!(result.is_err(), "相対パスは拒否する");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test commands::directory_commands`
Expected: コンパイルエラー

- [ ] **Step 3: 実装**

`commands/directory_commands.rs`:

```rust
use tauri::State;

use crate::classifier::ollama::OllamaClassifier;
use crate::db::{cloud_rules, directories, project_contexts, project_files, settings};
use crate::error::AppError;
use crate::models::directory::{CloudRule, ProjectContext, ProjectDirectory, ProjectFile};
use crate::project_context::{self, RescanOutcome};
use crate::state::DbState;
use rusqlite::Connection;

/// パスを検証して紐付ける（コマンド本体から分離してテスト可能に）。
pub(crate) fn validate_and_link(
    conn: &Connection,
    project_id: &str,
    path: &str,
) -> Result<ProjectDirectory, AppError> {
    if !std::path::Path::new(path).is_absolute() {
        return Err(AppError::DirectoryScan(format!(
            "absolute path required: {}",
            path
        )));
    }
    directories::link_directory(conn, project_id, path)
}

pub(crate) fn apply_cloud_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
    allow: Option<bool>,
) -> Result<(), AppError> {
    match allow {
        Some(allow) => cloud_rules::set_rule(conn, directory_id, scope, relative_path, allow),
        None => cloud_rules::delete_rule(conn, directory_id, scope, relative_path),
    }
}

#[tauri::command]
pub fn link_project_directory(
    db: State<DbState>,
    project_id: String,
    path: String,
) -> Result<ProjectDirectory, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    validate_and_link(&conn, &project_id, &path)
}

#[tauri::command]
pub fn unlink_project_directory(db: State<DbState>, project_id: String) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    directories::unlink_directory(&conn, &project_id)
}

#[tauri::command]
pub fn get_project_directory(
    db: State<DbState>,
    project_id: String,
) -> Result<Option<ProjectDirectory>, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    directories::get_directory_by_project(&conn, &project_id)
}

#[tauri::command]
pub async fn rescan_project_directory(
    db: State<'_, DbState>,
    project_id: String,
) -> Result<RescanOutcome, AppError> {
    let (endpoint, model) = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        (
            settings::get_or_default(&conn, "ollama_endpoint", "http://localhost:11434"),
            settings::get_or_default(&conn, "ollama_model", "llama3.1:8b"),
        )
    };
    let generator = OllamaClassifier::new(&endpoint, &model)?;
    // 現状ダイジェスト生成は Ollama（ローカル）のみのため cloud=false。
    // Claude 対応時は LLM プロバイダ設定に応じて true を渡す（スペック§5）。
    project_context::rescan_project(&db.0, &generator, &project_id, false).await
}

#[tauri::command]
pub fn list_project_files(
    db: State<DbState>,
    directory_id: String,
) -> Result<Vec<ProjectFile>, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    project_files::list_files(&conn, &directory_id)
}

#[tauri::command]
pub fn set_cloud_rule(
    db: State<DbState>,
    directory_id: String,
    scope: String,
    relative_path: String,
    allow: Option<bool>,
) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    apply_cloud_rule(&conn, &directory_id, &scope, &relative_path, allow)
}

#[tauri::command]
pub fn get_cloud_rules(
    db: State<DbState>,
    directory_id: String,
) -> Result<Vec<CloudRule>, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    cloud_rules::list_rules(&conn, &directory_id)
}

#[tauri::command]
pub fn set_allow_cloud_context(
    db: State<DbState>,
    project_id: String,
    allow: bool,
) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    project_contexts::set_allow_cloud_context(&conn, &project_id, allow)
}

#[tauri::command]
pub fn get_project_context(
    db: State<DbState>,
    project_id: String,
) -> Result<Option<ProjectContext>, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    project_contexts::get_context(&conn, &project_id)
}
```

`lib.rs` の `invoke_handler` に9コマンドを追加登録し、`setup` クロージャ内に起動時スキャンを追加:

```rust
            // 起動時バックグラウンドスキャン（スペック§4）
            {
                use tauri::Manager;
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let db = app_handle.state::<DbState>();
                    let targets: Vec<String> = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let mut stmt = match conn
                            .prepare("SELECT project_id FROM project_directories")
                        {
                            Ok(s) => s,
                            Err(_) => return,
                        };
                        stmt.query_map([], |row| row.get(0))
                            .map(|rows| rows.filter_map(|r| r.ok()).collect())
                            .unwrap_or_default()
                    };
                    if targets.is_empty() {
                        return;
                    }
                    let (endpoint, model) = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        (
                            db::settings::get_or_default(
                                &conn, "ollama_endpoint", "http://localhost:11434",
                            ),
                            db::settings::get_or_default(&conn, "ollama_model", "llama3.1:8b"),
                        )
                    };
                    let generator = match classifier::ollama::OllamaClassifier::new(&endpoint, &model) {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    for project_id in targets {
                        if let Err(e) = project_context::rescan_project(
                            &db.0, &generator, &project_id, false,
                        )
                        .await
                        {
                            eprintln!("[warn] startup scan failed for {}: {}", project_id, e);
                        }
                    }
                });
            }
```

- [ ] **Step 4: テスト・ビルドが通ることを確認**

Run: `cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: 全 PASS / clippy クリーン（PR4 の締め）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/commands/ src-tauri/src/lib.rs
git commit -m "feat(project-context): ディレクトリ連携のTauri commandsと起動時スキャンを追加"
```

---

## 補足: 実装後の周辺更新（Task 12 完了後、同PRで実施）

- `agent.md` のセキュリティルール改訂（スペック§5の文言をそのまま反映）:
  変更前「LLMへ送信するデータは件名、送信者、本文冒頭300文字に限定する」→
  変更後「LLMへ送信するデータは、件名・送信者・本文冒頭300文字、および案件ディレクトリ連携のコンテキスト（`docs/superpowers/specs/2026-07-09-project-directory-context-design.md` の送信可否ポリシーに従う）に限定する。クラウドLLMへのファイル由来データの送信はユーザーが明示的に許可したものに限る」
- コミット: `git commit -m "docs(agent): LLM送信データのルールをディレクトリ連携に合わせて改訂"`
