# 案件ノート（Project Notes） 実装プラン

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリ連携の有無に関わらず全案件が「案件ノート」を持てるようにし、アプリ内 WYSIWYG エディタで編集できるようにする。

**Architecture:** 案件ノートの正本を SQLite（新テーブル `project_notes`）に一本化する。ノートは `user_md`（ユーザー手書き）と `ai_md`（案件所属メールから AI 生成、ユーザー編集可）の2区画を Markdown で保持し、UI ではタブで分離する。ディレクトリ連携済み案件では既存の `PIGEON-CONTEXT.md` と双方向同期し、分類プロンプト注入（`project_contexts.cached_context`）は既存経路のまま維持する。

**Tech Stack:** Rust (Tauri 2, rusqlite, thiserror, async-trait) / React 19 + TypeScript + Zustand 5 + Tailwind v4 / TipTap 3.x / Vitest + React Testing Library / cargo test

**設計書:** `docs/design/2026-07-19-project-notes-design.md`

## Global Constraints

- Rust: `unwrap()` / `expect()` はテストコード以外で使用禁止。エラーは `thiserror` の `AppError`。Tauri command は `Result<T, AppError>` を返す（既存 `directory_commands.rs` に倣う。`AppError` は Serialize 実装済みで invoke に返せる）
- TypeScript: `any` 使用禁止。invoke レスポンスには必ず型を付け、共通型は `src/types/` に置く
- TDD: Red → Green → Refactor。テストを先に書き、失敗を確認してから実装する
- コミットは Conventional Commits 形式（`feat(scope): 説明` / `test(scope): 説明`）。scope は `project-notes` を使う
- 1コミット = 1意図。作業完了後の1コミットにまとめない
- **migration 番号**: 本プラン執筆時点の最新は v20。実装開始時に `src-tauri/src/db/migrations.rs` の `MIGRATIONS` 配列末尾を必ず確認し、次の空き番号を使うこと（並行作業で番号が進んでいる可能性がある。以下 `v21` と記載する箇所はすべてこの確認結果に読み替える）
- 既存 `project_contexts` テーブルは削除・改変しない（`cached_context` の生成元が変わるだけ）
- LLM 送信境界: 件名・送信者・本文冒頭1000文字まで（ADR-0002）。テストで実 LLM を呼ばない（モック `TextGenerator` を使う）

## 新規依存関係（Task 6 で追加）

設計書では「既存メール作成側の変換資産を流用」と書いたが、調査の結果 **既存 `src/components/compose/RichTextEditor.tsx` は HTML を保持しており Markdown 変換層は存在しない**。また表の拡張も未導入。したがって以下を新規追加する:

- `@tiptap/extension-table`, `@tiptap/extension-table-row`, `@tiptap/extension-table-cell`, `@tiptap/extension-table-header`（表サポート）
- `tiptap-markdown`（TipTap ドキュメント ⇔ Markdown 変換、GFM 表対応）

## File Structure

**Rust (src-tauri/src/)**
- `db/migrations.rs` — 修正: `migrate_v21` 追加 + `MIGRATIONS` 配列に1行追加
- `models/project_note.rs` — 新規: `ProjectNote` / `AiHistoryEntry` 型
- `models/mod.rs` — 修正: `pub mod project_note;` 追加
- `db/project_notes.rs` — 新規: `project_notes` / `project_note_ai_history` の CRUD
- `db/mod.rs` — 修正: `pub mod project_notes;` 追加
- `project_note_digest.rs` — 新規: メール群→AI要約の入力ビルダーとプロンプト
- `lib.rs` — 修正: `pub mod project_note_digest;` 追加 + invoke_handler にコマンド6本追加
- `commands/project_note_commands.rs` — 新規: Tauri コマンド
- `commands/mod.rs` — 修正: `pub mod project_note_commands;` 追加
- `project_context/mod.rs` — 修正: 自己修復を DB 正本前提に付け替え

**TypeScript (src/)**
- `types/projectNote.ts` — 新規: `ProjectNote` / `AiHistoryEntry` 型
- `api/projectNoteApi.ts` — 新規: invoke ラッパ
- `stores/projectNoteStore.ts` — 新規: Zustand ストア
- `utils/markdown.ts` — 新規: Markdown ⇔ TipTap 変換ヘルパ
- `components/project-note/ProjectNoteEditor.tsx` — 新規: TipTap ラッパ（表対応）
- `components/project-note/ProjectNotePanel.tsx` — 新規: タブ + 生成/履歴 UI

---

### Task 1: DB スキーマ（migration v21）

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`（`MIGRATIONS` 配列 157-177行付近、`migrate_v20` の後ろに関数追加）

**Interfaces:**
- Consumes: 既存の `Migration` 型 `(i32, fn(&Connection) -> Result<(), AppError>)`
- Produces: テーブル `project_notes(project_id, user_md, ai_md, ai_edited, ai_generated_at, updated_at)` と `project_note_ai_history(id, project_id, ai_md, replaced_at)`

- [ ] **Step 1: 実装開始前に migration 番号を確認**

```bash
grep -n "const MIGRATIONS" -A 30 src-tauri/src/db/migrations.rs | tail -15
```

配列末尾の番号 + 1 が使うべき番号。以下 `v21` はこの結果に読み替える。

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/db/migrations.rs` の `#[cfg(test)] mod tests` 内に追加:

```rust
    #[test]
    fn test_migration_creates_project_notes_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // 両テーブルが存在する
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'
                 AND name IN ('project_notes', 'project_note_ai_history')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // 案件削除で CASCADE する
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('a1', 'T', 't@e.com', 'i', 's', 'plain', 'other')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'a1', 'P')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_notes (project_id, user_md) VALUES ('p1', 'note')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_note_ai_history (id, project_id, ai_md)
             VALUES ('h1', 'p1', 'old')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM projects WHERE id = 'p1'", []).unwrap();

        let notes: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_notes", [], |r| r.get(0))
            .unwrap();
        let hist: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_note_ai_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(notes, 0, "案件削除で project_notes も消える");
        assert_eq!(hist, 0, "案件削除で履歴も消える");
    }

    #[test]
    fn test_project_notes_defaults() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('a1', 'T', 't@e.com', 'i', 's', 'plain', 'other')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'a1', 'P')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO project_notes (project_id) VALUES ('p1')", [])
            .unwrap();

        let (user_md, ai_md, ai_edited): (String, Option<String>, bool) = conn
            .query_row(
                "SELECT user_md, ai_md, ai_edited FROM project_notes WHERE project_id='p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(user_md, "");
        assert_eq!(ai_md, None);
        assert!(!ai_edited);
    }
```

- [ ] **Step 3: テストを実行して失敗を確認**

Run: `cd src-tauri && cargo test test_migration_creates_project_notes_tables -- --nocapture`
Expected: FAIL — `no such table: project_notes`

- [ ] **Step 4: migration を実装**

`migrate_v20` 関数の直後に追加:

```rust
/// 案件ノート。ディレクトリ連携の有無に関わらず全案件が持てる自由記述ノート。
/// 正本は DB 側（PIGEON-CONTEXT.md はディレクトリ連携時のミラー）。
fn migrate_v21(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS project_notes (
            project_id      TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            user_md         TEXT NOT NULL DEFAULT '',
            ai_md           TEXT,
            ai_edited       BOOLEAN NOT NULL DEFAULT FALSE,
            ai_generated_at DATETIME,
            updated_at      DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS project_note_ai_history (
            id          TEXT PRIMARY KEY,
            project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            ai_md       TEXT NOT NULL,
            replaced_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_project_note_ai_history_project
            ON project_note_ai_history(project_id);
        ",
    )?;
    Ok(())
}
```

`MIGRATIONS` 配列末尾に1行追加:

```rust
    (21, migrate_v21),
```

- [ ] **Step 5: テストを実行して成功を確認**

Run: `cd src-tauri && cargo test project_notes -- --nocapture`
Expected: PASS（`test_migration_creates_project_notes_tables`, `test_project_notes_defaults` 両方）

- [ ] **Step 6: 既存 migration テストが壊れていないことを確認**

Run: `cd src-tauri && cargo test migrations`
Expected: 全 PASS（`test_run_migrations_is_idempotent` を含む）

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/db/migrations.rs
git commit -m "feat(project-notes): project_notes と履歴テーブルのmigrationを追加"
```

---

### Task 2: モデル型と DB CRUD

**Files:**
- Create: `src-tauri/src/models/project_note.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Create: `src-tauri/src/db/project_notes.rs`
- Modify: `src-tauri/src/db/mod.rs`

**Interfaces:**
- Consumes: Task 1 のテーブル、`crate::test_helpers::setup_db`、`crate::error::AppError`
- Produces:
  - `models::project_note::ProjectNote { project_id: String, user_md: String, ai_md: Option<String>, ai_edited: bool, ai_generated_at: Option<String>, updated_at: Option<String> }`
  - `models::project_note::AiHistoryEntry { id: String, project_id: String, ai_md: String, replaced_at: String }`
  - `db::project_notes::get_note(&Connection, &str) -> Result<Option<ProjectNote>, AppError>`
  - `db::project_notes::upsert_user_md(&Connection, &str, &str) -> Result<(), AppError>`
  - `db::project_notes::upsert_ai_md(&Connection, &str, &str, bool) -> Result<(), AppError>`
  - `db::project_notes::replace_ai_md_with_history(&mut Connection, &str, &str) -> Result<(), AppError>`
  - `db::project_notes::list_ai_history(&Connection, &str) -> Result<Vec<AiHistoryEntry>, AppError>`
  - `db::project_notes::restore_ai_from_history(&mut Connection, &str) -> Result<(), AppError>`
  - `db::project_notes::AI_HISTORY_LIMIT: usize = 10`

- [ ] **Step 1: モデル型を作成**

`src-tauri/src/models/project_note.rs`:

```rust
use serde::{Deserialize, Serialize};

/// 案件ノート。正本は DB 側（PIGEON-CONTEXT.md はディレクトリ連携時のミラー）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectNote {
    pub project_id: String,
    /// 「ノート」タブ: ユーザー手書き（Markdown/GFM）
    pub user_md: String,
    /// 「AI要約」タブ: AI が生成した下書き。ユーザー編集可
    pub ai_md: Option<String>,
    /// ユーザーが ai_md を手修正したか（再生成時の確認ダイアログ判定に使う）
    pub ai_edited: bool,
    pub ai_generated_at: Option<String>,
    pub updated_at: Option<String>,
}

/// AI要約の再生成履歴。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiHistoryEntry {
    pub id: String,
    pub project_id: String,
    pub ai_md: String,
    pub replaced_at: String,
}
```

`src-tauri/src/models/mod.rs` に追加:

```rust
pub mod project_note;
```

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/db/project_notes.rs` を作成し、まずテストのみ書く:

```rust
use crate::error::AppError;
use crate::models::project_note::{AiHistoryEntry, ProjectNote};
use rusqlite::{params, Connection, OptionalExtension};

/// AI要約履歴の保持上限。これを超えた古い履歴は退避時に削除する。
pub const AI_HISTORY_LIMIT: usize = 10;

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
    fn test_get_note_none_initially() {
        let conn = setup_db();
        create_project(&conn);
        assert!(get_note(&conn, "p1").unwrap().is_none());
    }

    #[test]
    fn test_upsert_user_md_creates_row() {
        let conn = setup_db();
        create_project(&conn);
        upsert_user_md(&conn, "p1", "# 会場メモ").unwrap();
        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.user_md, "# 会場メモ");
        assert_eq!(note.ai_md, None);
        assert!(!note.ai_edited);
    }

    #[test]
    fn test_upsert_user_md_preserves_ai_md() {
        let conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "AI要約", false).unwrap();
        upsert_user_md(&conn, "p1", "手書き").unwrap();
        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.user_md, "手書き");
        assert_eq!(note.ai_md.as_deref(), Some("AI要約"), "ai_md は消えない");
    }

    #[test]
    fn test_upsert_ai_md_marks_edited() {
        let conn = setup_db();
        create_project(&conn);
        // AI生成時は edited=false
        upsert_ai_md(&conn, "p1", "生成結果", false).unwrap();
        assert!(!get_note(&conn, "p1").unwrap().unwrap().ai_edited);
        // ユーザー手編集時は edited=true
        upsert_ai_md(&conn, "p1", "手で直した", true).unwrap();
        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert!(note.ai_edited);
        assert_eq!(note.ai_md.as_deref(), Some("手で直した"));
    }

    #[test]
    fn test_replace_ai_md_moves_old_to_history() {
        let mut conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "旧要約", true).unwrap();

        replace_ai_md_with_history(&mut conn, "p1", "新要約").unwrap();

        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.ai_md.as_deref(), Some("新要約"));
        assert!(!note.ai_edited, "再生成後は edited がリセットされる");
        assert!(note.ai_generated_at.is_some());

        let hist = list_ai_history(&conn, "p1").unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].ai_md, "旧要約");
    }

    #[test]
    fn test_replace_ai_md_no_history_when_empty() {
        let mut conn = setup_db();
        create_project(&conn);
        // 既存 ai_md が無い初回生成では履歴を作らない
        replace_ai_md_with_history(&mut conn, "p1", "初回").unwrap();
        assert!(list_ai_history(&conn, "p1").unwrap().is_empty());
    }

    #[test]
    fn test_history_pruned_to_limit() {
        let mut conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "v0", false).unwrap();
        // AI_HISTORY_LIMIT を超える回数だけ再生成する
        for i in 1..=(AI_HISTORY_LIMIT + 3) {
            replace_ai_md_with_history(&mut conn, "p1", &format!("v{}", i)).unwrap();
        }
        let hist = list_ai_history(&conn, "p1").unwrap();
        assert_eq!(hist.len(), AI_HISTORY_LIMIT, "上限を超えて溜まらない");
    }

    #[test]
    fn test_restore_ai_from_history() {
        let mut conn = setup_db();
        create_project(&conn);
        upsert_ai_md(&conn, "p1", "旧要約", false).unwrap();
        replace_ai_md_with_history(&mut conn, "p1", "新要約").unwrap();

        let hist = list_ai_history(&conn, "p1").unwrap();
        let target = hist[0].id.clone();
        restore_ai_from_history(&mut conn, &target).unwrap();

        let note = get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.ai_md.as_deref(), Some("旧要約"), "履歴の内容が戻る");
    }

    #[test]
    fn test_restore_missing_history_errors() {
        let mut conn = setup_db();
        create_project(&conn);
        assert!(restore_ai_from_history(&mut conn, "nonexistent").is_err());
    }
}
```

- [ ] **Step 3: テストを実行して失敗を確認**

`src-tauri/src/db/mod.rs` に `pub mod project_notes;` を追加してから:

Run: `cd src-tauri && cargo test project_notes::tests`
Expected: FAIL — `cannot find function get_note in this scope`（コンパイルエラー）

- [ ] **Step 4: CRUD を実装**

`src-tauri/src/db/project_notes.rs` の `#[cfg(test)]` より上に追加:

```rust
fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectNote> {
    Ok(ProjectNote {
        project_id: row.get(0)?,
        user_md: row.get(1)?,
        ai_md: row.get(2)?,
        ai_edited: row.get(3)?,
        ai_generated_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub fn get_note(conn: &Connection, project_id: &str) -> Result<Option<ProjectNote>, AppError> {
    conn.query_row(
        "SELECT project_id, user_md, ai_md, ai_edited, ai_generated_at, updated_at
         FROM project_notes WHERE project_id = ?1",
        params![project_id],
        row_to_note,
    )
    .optional()
    .map_err(AppError::Database)
}

/// 「ノート」タブの保存。ai_md 側は触らない。
pub fn upsert_user_md(
    conn: &Connection,
    project_id: &str,
    user_md: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_notes (project_id, user_md, updated_at)
         VALUES (?1, ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            user_md = ?2, updated_at = CURRENT_TIMESTAMP",
        params![project_id, user_md],
    )?;
    Ok(())
}

/// 「AI要約」タブの保存。mark_edited=true はユーザー手編集を意味する。
pub fn upsert_ai_md(
    conn: &Connection,
    project_id: &str,
    ai_md: &str,
    mark_edited: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_notes (project_id, ai_md, ai_edited, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            ai_md = ?2, ai_edited = ?3, updated_at = CURRENT_TIMESTAMP",
        params![project_id, ai_md, mark_edited],
    )?;
    Ok(())
}

/// AI再生成。既存 ai_md があれば履歴へ退避してから上書きし、履歴を上限まで剪定する。
/// 退避と上書きは1トランザクションで行う（片方だけ成功する状態を作らない）。
pub fn replace_ai_md_with_history(
    conn: &mut Connection,
    project_id: &str,
    new_ai_md: &str,
) -> Result<(), AppError> {
    let tx = conn.transaction()?;

    let existing: Option<String> = tx
        .query_row(
            "SELECT ai_md FROM project_notes WHERE project_id = ?1",
            params![project_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(AppError::Database)?
        .flatten();

    if let Some(old) = existing {
        if !old.is_empty() {
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO project_note_ai_history (id, project_id, ai_md)
                 VALUES (?1, ?2, ?3)",
                params![id, project_id, old],
            )?;
            // 上限を超えた古い履歴を削除
            tx.execute(
                "DELETE FROM project_note_ai_history
                 WHERE project_id = ?1 AND id NOT IN (
                     SELECT id FROM project_note_ai_history
                     WHERE project_id = ?1
                     ORDER BY replaced_at DESC, rowid DESC
                     LIMIT ?2
                 )",
                params![project_id, AI_HISTORY_LIMIT as i64],
            )?;
        }
    }

    tx.execute(
        "INSERT INTO project_notes
            (project_id, ai_md, ai_edited, ai_generated_at, updated_at)
         VALUES (?1, ?2, FALSE, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            ai_md = ?2, ai_edited = FALSE,
            ai_generated_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP",
        params![project_id, new_ai_md],
    )?;

    tx.commit()?;
    Ok(())
}

pub fn list_ai_history(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<AiHistoryEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, ai_md, replaced_at
         FROM project_note_ai_history
         WHERE project_id = ?1
         ORDER BY replaced_at DESC, rowid DESC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(AiHistoryEntry {
            id: row.get(0)?,
            project_id: row.get(1)?,
            ai_md: row.get(2)?,
            replaced_at: row.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 履歴から ai_md を復元する。復元自体も再生成扱いで現在値を履歴へ退避する。
pub fn restore_ai_from_history(conn: &mut Connection, history_id: &str) -> Result<(), AppError> {
    let (project_id, ai_md): (String, String) = conn
        .query_row(
            "SELECT project_id, ai_md FROM project_note_ai_history WHERE id = ?1",
            params![history_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::Validation(format!("history not found: {}", history_id)))?;

    replace_ai_md_with_history(conn, &project_id, &ai_md)
}
```

`src-tauri/src/db/mod.rs` に追加（Step 3 で未追加なら）:

```rust
pub mod project_notes;
```

- [ ] **Step 5: テストを実行して成功を確認**

Run: `cd src-tauri && cargo test project_notes::tests`
Expected: 全9テスト PASS

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/models/project_note.rs src-tauri/src/models/mod.rs \
        src-tauri/src/db/project_notes.rs src-tauri/src/db/mod.rs
git commit -m "feat(project-notes): 案件ノートのモデル型とDB CRUDを追加"
```

---

### Task 3: メール群からの AI 要約生成（入力ビルダー + プロンプト）

**Files:**
- Create: `src-tauri/src/project_note_digest.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod project_note_digest;` を他の `pub mod` 宣言の並びに追加）

**Interfaces:**
- Consumes: `crate::classifier::TextGenerator`（`generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError>`）、`crate::models::mail::Mail`
- Produces:
  - `project_note_digest::MAIL_DIGEST_SYSTEM_PROMPT: &str`
  - `project_note_digest::MAX_MAILS: usize = 50`
  - `project_note_digest::BODY_HEAD_CHARS: usize = 1000`
  - `project_note_digest::build_mail_digest_input(project_name: &str, mails: &[Mail]) -> (String, usize)` — 戻り値の `usize` は切り捨てた件数
  - `project_note_digest::generate_mail_digest(generator: &dyn TextGenerator, input: &str) -> Result<String, AppError>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/project_note_digest.rs` を作成:

```rust
use crate::classifier::TextGenerator;
use crate::error::AppError;
use crate::models::mail::Mail;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_mail;
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

    fn mail_with_body(id: &str, subject: &str, from: &str, body: &str) -> Mail {
        let mut m = make_mail(id, &format!("<{}@e>", id), subject, "2026-07-19T10:00:00Z");
        m.from_addr = from.to_string();
        m.body_text = Some(body.to_string());
        m
    }

    #[test]
    fn test_build_input_includes_subject_and_from() {
        let mails = vec![mail_with_body("m1", "搬入の件", "a@example.com", "本文です")];
        let (input, dropped) = build_mail_digest_input("春公演", &mails);
        assert!(input.contains("春公演"));
        assert!(input.contains("搬入の件"));
        assert!(input.contains("a@example.com"));
        assert!(input.contains("本文です"));
        assert_eq!(dropped, 0);
    }

    #[test]
    fn test_build_input_truncates_body_at_boundary() {
        let long_body = "あ".repeat(BODY_HEAD_CHARS + 500);
        let mails = vec![mail_with_body("m1", "件名", "a@example.com", &long_body)];
        let (input, _) = build_mail_digest_input("P", &mails);
        let body_chars = input.matches('あ').count();
        assert_eq!(
            body_chars, BODY_HEAD_CHARS,
            "本文は冒頭1000文字までしか含めない（ADR-0002の送信境界）"
        );
    }

    #[test]
    fn test_build_input_caps_mail_count_and_reports_dropped() {
        let mails: Vec<Mail> = (0..(MAX_MAILS + 7))
            .map(|i| mail_with_body(&format!("m{}", i), "件名", "a@example.com", "本文"))
            .collect();
        let (input, dropped) = build_mail_digest_input("P", &mails);
        assert_eq!(dropped, 7, "超過分の件数を返す（サイレント切り捨て禁止）");
        assert_eq!(input.matches("### メール").count(), MAX_MAILS);
    }

    #[test]
    fn test_build_input_handles_missing_body() {
        let mut m = make_mail("m1", "<m1@e>", "件名のみ", "2026-07-19T10:00:00Z");
        m.body_text = None;
        let (input, _) = build_mail_digest_input("P", &[m]);
        assert!(input.contains("件名のみ"), "本文が無くても件名は含まれる");
    }

    #[tokio::test]
    async fn test_generate_mail_digest_returns_llm_output() {
        let gen = MockGenerator {
            response: "- 公演: 春公演\n- 会場: 〇〇ホール".to_string(),
        };
        let out = generate_mail_digest(&gen, "入力").await.unwrap();
        assert!(out.contains("春公演"));
    }
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

`src-tauri/src/lib.rs` に `pub mod project_note_digest;` を追加してから:

Run: `cd src-tauri && cargo test project_note_digest`
Expected: FAIL — `cannot find function build_mail_digest_input`（コンパイルエラー）

- [ ] **Step 3: 実装する**

`src-tauri/src/project_note_digest.rs` の `#[cfg(test)]` より上に追加:

```rust
/// AI要約に使うメール件数の上限。超過分は切り捨て、件数を呼び出し元へ返す。
pub const MAX_MAILS: usize = 50;
/// 1通あたりの本文送信上限（ADR-0002 のクラウド送信境界と同一）。
pub const BODY_HEAD_CHARS: usize = 1000;

pub const MAIL_DIGEST_SYSTEM_PROMPT: &str = "\
あなたは舞台制作の案件アシスタントです。案件に属するメールのやり取りから、
この案件の要約を Markdown の箇条書きで出力してください。

出力形式（この形式のみ、前置き・後置きなし）:
- 公演: <公演名・演目>
- 会場: <会場名とキーワード>
- 関係する組織・人: <メールから読み取れる関係先>
- キーワード: <メール分類の手がかりになる語>
- 主なやり取り: <論点・決定事項を3件まで>

読み取れない項目は行ごと省略する。推測で埋めない。全体で400字以内。";

/// メール群から LLM への入力を組み立てる。
/// 戻り値は (入力文字列, 切り捨てたメール件数)。
/// 送信するのは件名・送信者・本文冒頭 BODY_HEAD_CHARS 文字のみ（ADR-0002）。
pub fn build_mail_digest_input(project_name: &str, mails: &[Mail]) -> (String, usize) {
    let dropped = mails.len().saturating_sub(MAX_MAILS);
    let used = if mails.len() > MAX_MAILS {
        &mails[..MAX_MAILS]
    } else {
        mails
    };

    let mut input = format!("## 案件名\n{}\n\n", project_name);
    for (i, m) in used.iter().enumerate() {
        input.push_str(&format!("### メール{}\n", i + 1));
        input.push_str(&format!("- 件名: {}\n", m.subject));
        input.push_str(&format!("- 送信者: {}\n", m.from_addr));
        if let Some(body) = &m.body_text {
            let head: String = body.chars().take(BODY_HEAD_CHARS).collect();
            input.push_str(&format!("- 本文冒頭:\n{}\n", head));
        }
        input.push('\n');
    }
    (input, dropped)
}

pub async fn generate_mail_digest(
    generator: &dyn TextGenerator,
    input: &str,
) -> Result<String, AppError> {
    generator
        .generate_text(MAIL_DIGEST_SYSTEM_PROMPT, input)
        .await
}
```

**注意:** `Mail` 構造体のフィールド名（`subject` / `from_addr` / `body_text`）は実装前に `src-tauri/src/models/mail.rs` で確認すること。異なる場合はテストと実装の両方を実際の名前に合わせる。

- [ ] **Step 4: テストを実行して成功を確認**

Run: `cd src-tauri && cargo test project_note_digest`
Expected: 全5テスト PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/project_note_digest.rs src-tauri/src/lib.rs
git commit -m "feat(project-notes): メール群からAI要約を生成する入力ビルダーを追加"
```

---

### Task 4: PIGEON-CONTEXT.md との同期ヘルパ

**Files:**
- Create: `src-tauri/src/project_notes_sync.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod project_notes_sync;` 追加）

**Interfaces:**
- Consumes: `project_context::context_file::{split_at_marker, upsert_auto_section, build_cached_context, read_context_file, write_context_file, MAX_CACHED_CONTEXT_CHARS}`、`db::project_notes`、`db::project_contexts::update_cache_only`、`db::directories::get_directory_by_project`、`project_context::extractor::sha256_hex`
- Produces:
  - `project_notes_sync::compose_markdown(user_md: &str, ai_md: Option<&str>, project_name: &str) -> String`
  - `project_notes_sync::decompose_markdown(full_md: &str) -> (String, Option<String>)`
  - `project_notes_sync::sync_note_to_disk(&Connection, project_id: &str) -> Result<(), AppError>`
  - `project_notes_sync::refresh_cached_context(&Connection, project_id: &str) -> Result<(), AppError>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/project_notes_sync.rs` を作成:

```rust
use crate::db::{directories, project_contexts, project_notes};
use crate::error::AppError;
use crate::project_context::context_file::{
    build_cached_context, read_context_file, split_at_marker, upsert_auto_section,
    write_context_file, MAX_CACHED_CONTEXT_CHARS,
};
use crate::project_context::extractor::sha256_hex;
use rusqlite::Connection;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', '春公演')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_compose_then_decompose_roundtrip() {
        let composed = compose_markdown("# 手書き\n会場担当: 伊藤", Some("- 公演: 春公演"), "春公演");
        let (user, ai) = decompose_markdown(&composed);
        assert!(user.contains("# 手書き"));
        assert!(user.contains("会場担当: 伊藤"));
        assert_eq!(ai.as_deref().map(str::trim), Some("- 公演: 春公演"));
    }

    #[test]
    fn test_compose_without_ai_section() {
        let composed = compose_markdown("手書きのみ", None, "春公演");
        let (user, _ai) = decompose_markdown(&composed);
        assert!(user.contains("手書きのみ"));
    }

    #[test]
    fn test_decompose_file_without_marker_is_all_user() {
        // ユーザーが外部エディタで自作した（マーカー無し）ファイル
        let (user, ai) = decompose_markdown("# 自作メモ\n大事なこと\n");
        assert!(user.contains("# 自作メモ"));
        assert_eq!(ai, None, "マーカー無しは全部ユーザー欄");
    }

    #[test]
    fn test_sync_to_disk_writes_file_when_linked() {
        let mut conn = setup_db();
        create_project(&conn);
        let dir = tempfile::tempdir().unwrap();
        directories::link_directory(&mut conn, "p1", dir.path().to_str().unwrap()).unwrap();

        project_notes::upsert_user_md(&conn, "p1", "会場担当: 伊藤").unwrap();
        project_notes::upsert_ai_md(&conn, "p1", "- 公演: 春公演", false).unwrap();

        sync_note_to_disk(&conn, "p1").unwrap();

        let written = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(written.contains("会場担当: 伊藤"));
        assert!(written.contains("- 公演: 春公演"));
    }

    #[test]
    fn test_sync_to_disk_noop_when_not_linked() {
        let conn = setup_db();
        create_project(&conn);
        project_notes::upsert_user_md(&conn, "p1", "ノート").unwrap();
        // ディレクトリ未連携でもエラーにならない（何もしない）
        sync_note_to_disk(&conn, "p1").unwrap();
    }

    #[test]
    fn test_refresh_cached_context_prioritizes_user_section() {
        let conn = setup_db();
        create_project(&conn);
        project_notes::upsert_user_md(&conn, "p1", "ユーザー欄の内容").unwrap();
        project_notes::upsert_ai_md(&conn, "p1", "AI欄の内容", false).unwrap();

        refresh_cached_context(&conn, "p1").unwrap();

        let ctx = project_contexts::get_context(&conn, "p1").unwrap().unwrap();
        let cached = ctx.cached_context.unwrap();
        assert!(cached.contains("ユーザー欄の内容"));
        assert!(cached.chars().count() <= MAX_CACHED_CONTEXT_CHARS);
    }
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

`src-tauri/src/lib.rs` に `pub mod project_notes_sync;` を追加してから:

Run: `cd src-tauri && cargo test project_notes_sync`
Expected: FAIL — `cannot find function compose_markdown`（コンパイルエラー）

- [ ] **Step 3: 実装する**

`#[cfg(test)]` より上に追加:

```rust
/// user_md + ai_md を1本の PIGEON-CONTEXT.md 形式へ合成する。
/// 既存の upsert_auto_section の規約（マーカーより上がユーザー欄）に合わせる。
pub fn compose_markdown(user_md: &str, ai_md: Option<&str>, project_name: &str) -> String {
    let existing_user = if user_md.trim().is_empty() {
        None
    } else {
        Some(user_md)
    };
    upsert_auto_section(existing_user, project_name, ai_md.unwrap_or(""))
}

/// PIGEON-CONTEXT.md 形式を (user_md, ai_md) へ分解する。
/// マーカー無しのファイル（ユーザーの自作）は全体をユーザー欄として扱う。
pub fn decompose_markdown(full_md: &str) -> (String, Option<String>) {
    split_at_marker(full_md)
}

/// 案件ノートをディレクトリの PIGEON-CONTEXT.md へ書き出す（DB→ファイルのミラー）。
/// ディレクトリ未連携の案件では何もしない。
pub fn sync_note_to_disk(conn: &Connection, project_id: &str) -> Result<(), AppError> {
    let dir = match directories::get_directory_by_project(conn, project_id)? {
        Some(d) => d,
        None => return Ok(()),
    };
    let note = match project_notes::get_note(conn, project_id)? {
        Some(n) => n,
        None => return Ok(()),
    };
    let project_name = crate::db::projects::get_project(conn, project_id)?.name;
    let composed = compose_markdown(&note.user_md, note.ai_md.as_deref(), &project_name);
    write_context_file(Path::new(&dir.path), &composed)
}

/// 案件ノートから分類プロンプト注入用キャッシュ (project_contexts.cached_context) を再生成する。
pub fn refresh_cached_context(conn: &Connection, project_id: &str) -> Result<(), AppError> {
    let note = match project_notes::get_note(conn, project_id)? {
        Some(n) => n,
        None => return Ok(()),
    };
    let project_name = crate::db::projects::get_project(conn, project_id)?.name;
    let composed = compose_markdown(&note.user_md, note.ai_md.as_deref(), &project_name);
    let cached = build_cached_context(&composed, MAX_CACHED_CONTEXT_CHARS);
    let hash = sha256_hex(composed.as_bytes());
    project_contexts::update_cache_only(conn, project_id, &cached, &hash)
}

/// ディレクトリ上の PIGEON-CONTEXT.md をDBへ取り込む（ファイル→DB、自己修復・初期移行用）。
/// 外部エディタでの編集を DB 正本へ反映する。
pub fn import_note_from_disk(conn: &Connection, project_id: &str) -> Result<bool, AppError> {
    let dir = match directories::get_directory_by_project(conn, project_id)? {
        Some(d) => d,
        None => return Ok(false),
    };
    let full_md = match read_context_file(Path::new(&dir.path))? {
        Some(md) => md,
        None => return Ok(false),
    };
    let (user_md, ai_md) = decompose_markdown(&full_md);
    project_notes::upsert_user_md(conn, project_id, user_md.trim())?;
    if let Some(ai) = ai_md {
        // ファイル由来の取り込みは「AI生成そのまま」とみなし edited は立てない
        project_notes::upsert_ai_md(conn, project_id, ai.trim(), false)?;
    }
    Ok(true)
}
```

**注意:** `db::projects::get_project` の戻り値型と `directories::get_directory_by_project` / `link_directory` のシグネチャは実装前に確認すること（`link_directory` は `&mut Connection` を取る）。

- [ ] **Step 4: テストを実行して成功を確認**

Run: `cd src-tauri && cargo test project_notes_sync`
Expected: 全6テスト PASS

- [ ] **Step 5: `import_note_from_disk` のテストを追加**

Step 1 のテストモジュールに追加:

```rust
    #[test]
    fn test_import_from_disk_splits_into_two_columns() {
        let mut conn = setup_db();
        create_project(&conn);
        let dir = tempfile::tempdir().unwrap();
        directories::link_directory(&mut conn, "p1", dir.path().to_str().unwrap()).unwrap();

        let file_content = format!(
            "# 手書きタイトル\n担当: 伊藤\n\n{}\n- 公演: 春公演\n",
            crate::project_context::context_file::AUTO_MARKER
        );
        std::fs::write(dir.path().join("PIGEON-CONTEXT.md"), &file_content).unwrap();

        assert!(import_note_from_disk(&conn, "p1").unwrap());

        let note = project_notes::get_note(&conn, "p1").unwrap().unwrap();
        assert!(note.user_md.contains("担当: 伊藤"));
        assert_eq!(note.ai_md.as_deref(), Some("- 公演: 春公演"));
        assert!(!note.ai_edited);
    }
```

- [ ] **Step 6: テストを実行して成功を確認**

Run: `cd src-tauri && cargo test project_notes_sync`
Expected: 全7テスト PASS

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/project_notes_sync.rs src-tauri/src/lib.rs
git commit -m "feat(project-notes): PIGEON-CONTEXT.mdとの双方向同期ヘルパを追加"
```

---

### Task 5: Tauri コマンド

**Files:**
- Create: `src-tauri/src/commands/project_note_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`（`invoke_handler` の `generate_handler!` 配列にコマンド6本を追加。既存の `commands::directory_commands::get_project_context` の並び付近）

**Interfaces:**
- Consumes: Task 2 の `db::project_notes`、Task 3 の `project_note_digest`、Task 4 の `project_notes_sync`、既存の `DbState`（`with_conn`）と LLM 生成器の取得方法
- Produces（フロントが呼ぶコマンド名）:
  - `get_project_note(project_id: String) -> Option<ProjectNote>`
  - `save_project_note_user(project_id: String, user_md: String) -> ()`
  - `save_project_note_ai(project_id: String, ai_md: String) -> ()`
  - `generate_project_note_ai(project_id: String, cloud: bool) -> GenerateNoteOutcome`
  - `list_project_note_ai_history(project_id: String) -> Vec<AiHistoryEntry>`
  - `restore_project_note_ai(history_id: String) -> ()`
  - `GenerateNoteOutcome { ai_md: String, dropped_mails: usize }`

- [ ] **Step 1: 既存の非同期コマンド（LLM 使用）の書き方を確認**

Run: `grep -n "rescan_project_directory" -A 25 src-tauri/src/commands/directory_commands.rs`

`rescan_project_directory` が LLM 生成器をどう取得し、ロックをどう扱っているかを読む。**本タスクの `generate_project_note_ai` は同じ様式に従う**（ロックは短く取り、LLM 呼び出しはロック外）。

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/commands/project_note_commands.rs` を作成し、まず内部ロジック関数のテストを書く:

```rust
use crate::db::project_notes;
use crate::error::AppError;
use crate::models::project_note::{AiHistoryEntry, ProjectNote};
use crate::project_notes_sync;
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct GenerateNoteOutcome {
    pub ai_md: String,
    /// 上限超過で AI 入力から除外したメール件数（0 なら全件使用）
    pub dropped_mails: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::project_notes;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_save_user_note_refreshes_cached_context() {
        let conn = setup_db();
        create_project(&conn);

        save_user_note_inner(&conn, "p1", "会場は〇〇ホール").unwrap();

        let note = project_notes::get_note(&conn, "p1").unwrap().unwrap();
        assert_eq!(note.user_md, "会場は〇〇ホール");

        // 分類プロンプト用キャッシュも更新される
        let ctx = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(ctx.cached_context.unwrap().contains("会場は〇〇ホール"));
    }

    #[test]
    fn test_save_ai_note_marks_edited() {
        let conn = setup_db();
        create_project(&conn);
        save_ai_note_inner(&conn, "p1", "手で直したAI要約").unwrap();
        let note = project_notes::get_note(&conn, "p1").unwrap().unwrap();
        assert!(note.ai_edited, "手編集保存は ai_edited を立てる");
    }
}
```

- [ ] **Step 3: テストを実行して失敗を確認**

`src-tauri/src/commands/mod.rs` に `pub mod project_note_commands;` を追加してから:

Run: `cd src-tauri && cargo test project_note_commands`
Expected: FAIL — `cannot find function save_user_note_inner`（コンパイルエラー）

- [ ] **Step 4: 内部ロジック関数とコマンドを実装**

`#[cfg(test)]` より上に追加:

```rust
/// 「ノート」タブ保存の実体。保存 → キャッシュ再生成 → ディスク同期。
/// ディスク書き込み失敗は DB 正本を巻き戻さず警告に留める（設計書 §7）。
fn save_user_note_inner(
    conn: &Connection,
    project_id: &str,
    user_md: &str,
) -> Result<(), AppError> {
    project_notes::upsert_user_md(conn, project_id, user_md)?;
    project_notes_sync::refresh_cached_context(conn, project_id)?;
    if let Err(e) = project_notes_sync::sync_note_to_disk(conn, project_id) {
        log::warn!("PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
    }
    Ok(())
}

/// 「AI要約」タブの手編集保存の実体。ai_edited を立てる。
fn save_ai_note_inner(conn: &Connection, project_id: &str, ai_md: &str) -> Result<(), AppError> {
    project_notes::upsert_ai_md(conn, project_id, ai_md, true)?;
    project_notes_sync::refresh_cached_context(conn, project_id)?;
    if let Err(e) = project_notes_sync::sync_note_to_disk(conn, project_id) {
        log::warn!("PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
    }
    Ok(())
}

#[tauri::command]
pub fn get_project_note(
    db: State<crate::DbState>,
    project_id: String,
) -> Result<Option<ProjectNote>, AppError> {
    db.with_conn(|conn| project_notes::get_note(conn, &project_id))
}

#[tauri::command]
pub fn save_project_note_user(
    db: State<crate::DbState>,
    project_id: String,
    user_md: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| save_user_note_inner(conn, &project_id, &user_md))
}

#[tauri::command]
pub fn save_project_note_ai(
    db: State<crate::DbState>,
    project_id: String,
    ai_md: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| save_ai_note_inner(conn, &project_id, &ai_md))
}

#[tauri::command]
pub fn list_project_note_ai_history(
    db: State<crate::DbState>,
    project_id: String,
) -> Result<Vec<AiHistoryEntry>, AppError> {
    db.with_conn(|conn| project_notes::list_ai_history(conn, &project_id))
}
```

**注意:** `DbState` の実際の型パスと `with_conn` のシグネチャ、`&mut Connection` を要する関数（`replace_ai_md_with_history` / `restore_ai_from_history`）を State から取り出す方法は、Step 1 で読んだ既存コマンドの様式に合わせること。`restore_project_note_ai` と `generate_project_note_ai` は `&mut Connection` が必要なため、既存の可変接続を取る様式（`directory_commands.rs` 内の `link_directory` 呼び出し箇所）に倣う。

- [ ] **Step 5: AI 生成コマンドを実装**

`generate_project_note_ai` は LLM を呼ぶため非同期。`rescan_project`（`project_context/mod.rs:27`）と同じ様式（ロックを短く取り、LLM 呼び出しはロック外）:

```rust
/// 案件所属メールから AI 要約を生成し、既存要約を履歴へ退避して差し替える。
/// メール0件の場合は生成せずエラーを返す。
#[tauri::command]
pub async fn generate_project_note_ai(
    db: State<'_, crate::DbState>,
    generator: State<'_, crate::LlmState>,
    project_id: String,
    cloud: bool,
) -> Result<GenerateNoteOutcome, AppError> {
    // 1. スナップショット取得（ロック内）
    let (project_name, mails) = {
        let conn = db.lock()?;
        let project = crate::db::projects::get_project(&conn, &project_id)?;
        let mails = crate::db::assignments::get_mails_by_project(&conn, &project_id)?;
        (project.name, mails)
    };

    if mails.is_empty() {
        return Err(AppError::Validation(
            "この案件にはメールがないため要約を生成できません".into(),
        ));
    }

    // 2. 入力組み立て + LLM 呼び出し（ロック外）
    let (input, dropped) =
        crate::project_note_digest::build_mail_digest_input(&project_name, &mails);
    if dropped > 0 {
        log::info!(
            "案件 {} のAI要約: メール {} 件中 {} 件を上限超過で除外",
            project_id,
            mails.len(),
            dropped
        );
    }
    let gen = generator.resolve(cloud)?;
    let ai_md = crate::project_note_digest::generate_mail_digest(gen.as_ref(), &input).await?;

    // 3. 書き込み（ロック内）。既存要約は履歴へ退避される
    {
        let mut conn = db.lock()?;
        project_notes::replace_ai_md_with_history(&mut conn, &project_id, &ai_md)?;
        project_notes_sync::refresh_cached_context(&conn, &project_id)?;
        if let Err(e) = project_notes_sync::sync_note_to_disk(&conn, &project_id) {
            log::warn!("PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
        }
    }

    Ok(GenerateNoteOutcome {
        ai_md,
        dropped_mails: dropped,
    })
}

#[tauri::command]
pub fn restore_project_note_ai(
    db: State<crate::DbState>,
    history_id: String,
) -> Result<(), AppError> {
    db.with_conn_mut(|conn| project_notes::restore_ai_from_history(conn, &history_id))
}
```

**注意:** `crate::LlmState` / `generator.resolve(cloud)` / `db.lock()` / `db.with_conn_mut` は仮の名前。Step 1 で読んだ `rescan_project_directory` の実際の State 型・生成器取得方法・ロック取得方法に必ず合わせること。既存に `with_conn_mut` が無ければ既存の可変接続取得様式を使う。

- [ ] **Step 6: `invoke_handler` に登録**

`src-tauri/src/lib.rs` の `tauri::generate_handler![...]` 内、`commands::directory_commands::get_project_context,` の近くに追加:

```rust
            commands::project_note_commands::get_project_note,
            commands::project_note_commands::save_project_note_user,
            commands::project_note_commands::save_project_note_ai,
            commands::project_note_commands::generate_project_note_ai,
            commands::project_note_commands::list_project_note_ai_history,
            commands::project_note_commands::restore_project_note_ai,
```

- [ ] **Step 7: テストとビルドを確認**

Run: `cd src-tauri && cargo test project_note_commands && cargo build`
Expected: テスト PASS かつビルド成功

- [ ] **Step 8: コミット**

```bash
git add src-tauri/src/commands/project_note_commands.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(project-notes): 案件ノートのTauriコマンドを追加"
```

---

### Task 6: フロント依存追加と Markdown 変換層

**Files:**
- Modify: `package.json`
- Create: `src/utils/markdown.ts`
- Create: `src/__tests__/utils/markdown.test.ts`
- Create: `src/types/projectNote.ts`

**Interfaces:**
- Produces:
  - `types/projectNote.ts`: `ProjectNote`, `AiHistoryEntry`, `GenerateNoteOutcome`
  - `utils/markdown.ts`: `NOTE_EXTENSIONS`（TipTap 拡張配列）

- [ ] **Step 1: 依存を追加**

```bash
pnpm add @tiptap/extension-table @tiptap/extension-table-row @tiptap/extension-table-cell @tiptap/extension-table-header tiptap-markdown
```

- [ ] **Step 2: 型定義を作成**

`src/types/projectNote.ts`:

```typescript
export interface ProjectNote {
  project_id: string;
  user_md: string;
  ai_md: string | null;
  ai_edited: boolean;
  ai_generated_at: string | null;
  updated_at: string | null;
}

export interface AiHistoryEntry {
  id: string;
  project_id: string;
  ai_md: string;
  replaced_at: string;
}

export interface GenerateNoteOutcome {
  ai_md: string;
  dropped_mails: number;
}
```

- [ ] **Step 3: 失敗するテストを書く**

`src/__tests__/utils/markdown.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { Editor } from "@tiptap/core";
import { NOTE_EXTENSIONS } from "../../utils/markdown";

function roundtrip(md: string): string {
  const editor = new Editor({ extensions: NOTE_EXTENSIONS, content: "" });
  // tiptap-markdown が提供する storage 経由で Markdown を読み書きする
  editor.commands.setContent(md);
  const out = editor.storage.markdown.getMarkdown();
  editor.destroy();
  return out;
}

describe("markdown roundtrip", () => {
  it("見出しと強調を保持する", () => {
    const out = roundtrip("# 春公演\n\n**会場担当**: 伊藤\n");
    expect(out).toContain("# 春公演");
    expect(out).toContain("**会場担当**");
  });

  it("箇条書きを保持する", () => {
    const out = roundtrip("- 搬入 9:00\n- リハ 13:00\n");
    expect(out).toContain("- 搬入 9:00");
    expect(out).toContain("- リハ 13:00");
  });

  it("表を保持する", () => {
    const md = "| 時刻 | 内容 |\n| --- | --- |\n| 9:00 | 搬入 |\n";
    const out = roundtrip(md);
    expect(out).toContain("9:00");
    expect(out).toContain("搬入");
    expect(out).toContain("|");
  });

  it("空文字を扱える", () => {
    expect(roundtrip("")).toBe("");
  });
});
```

- [ ] **Step 4: テストを実行して失敗を確認**

Run: `pnpm vitest run src/__tests__/utils/markdown.test.ts`
Expected: FAIL — `Cannot find module '../../utils/markdown'`

- [ ] **Step 5: 変換層を実装**

`src/utils/markdown.ts`:

```typescript
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";
import Table from "@tiptap/extension-table";
import TableRow from "@tiptap/extension-table-row";
import TableCell from "@tiptap/extension-table-cell";
import TableHeader from "@tiptap/extension-table-header";
import { Markdown } from "tiptap-markdown";

/**
 * 案件ノート用の TipTap 拡張セット。
 * 見出し・太字・斜体・箇条書き・番号リスト・リンク・表をサポートする。
 * 画像は設計上サポートしない（設計書 2026-07-19-project-notes-design.md §2）。
 */
export const NOTE_EXTENSIONS = [
  StarterKit,
  Link.configure({ openOnClick: false }),
  Table.configure({ resizable: false }),
  TableRow,
  TableHeader,
  TableCell,
  Markdown.configure({ html: false, breaks: true, transformPastedText: true }),
];
```

- [ ] **Step 6: テストを実行して成功を確認**

Run: `pnpm vitest run src/__tests__/utils/markdown.test.ts`
Expected: 全4テスト PASS

表のラウンドトリップが失敗する場合は `tiptap-markdown` の GFM table 設定を確認し、必要なら `Markdown.configure({ ... })` のオプションを調整する。**表サポートは要件なので、通らないまま次へ進まないこと。**

- [ ] **Step 7: コミット**

```bash
git add package.json pnpm-lock.yaml src/utils/markdown.ts src/types/projectNote.ts src/__tests__/utils/markdown.test.ts
git commit -m "feat(project-notes): Markdown変換層と表対応のTipTap拡張を追加"
```

---

### Task 7: API ラッパと Zustand ストア

**Files:**
- Create: `src/api/projectNoteApi.ts`
- Create: `src/stores/projectNoteStore.ts`
- Create: `src/__tests__/stores/projectNoteStore.test.ts`

**Interfaces:**
- Consumes: Task 5 のコマンド名、Task 6 の型
- Produces:
  - `api/projectNoteApi.ts`: `fetchProjectNote`, `saveUserNote`, `saveAiNote`, `generateAiNote`, `fetchAiHistory`, `restoreAiNote`
  - `stores/projectNoteStore.ts`: `useProjectNoteStore` with `{ note, history, loading, generating, error, load, saveUser, saveAi, generate, loadHistory, restore }`

- [ ] **Step 1: API ラッパを作成**

既存の `src/api/directoryApi.ts` の書き方に合わせる（先に `cat src/api/directoryApi.ts` で確認）。

`src/api/projectNoteApi.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import type {
  AiHistoryEntry,
  GenerateNoteOutcome,
  ProjectNote,
} from "../types/projectNote";

export function fetchProjectNote(projectId: string): Promise<ProjectNote | null> {
  return invoke<ProjectNote | null>("get_project_note", { projectId });
}

export function saveUserNote(projectId: string, userMd: string): Promise<void> {
  return invoke<void>("save_project_note_user", { projectId, userMd });
}

export function saveAiNote(projectId: string, aiMd: string): Promise<void> {
  return invoke<void>("save_project_note_ai", { projectId, aiMd });
}

export function generateAiNote(
  projectId: string,
  cloud: boolean,
): Promise<GenerateNoteOutcome> {
  return invoke<GenerateNoteOutcome>("generate_project_note_ai", { projectId, cloud });
}

export function fetchAiHistory(projectId: string): Promise<AiHistoryEntry[]> {
  return invoke<AiHistoryEntry[]>("list_project_note_ai_history", { projectId });
}

export function restoreAiNote(historyId: string): Promise<void> {
  return invoke<void>("restore_project_note_ai", { historyId });
}
```

**注意:** Tauri の invoke 引数は snake_case / camelCase の変換規約がある。既存 `directoryApi.ts` がどちらを使っているか確認し、必ず合わせること。

- [ ] **Step 2: 失敗するストアテストを書く**

`src/__tests__/stores/projectNoteStore.test.ts`:

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useProjectNoteStore } from "../../stores/projectNoteStore";

vi.mock("../../api/projectNoteApi", () => ({
  fetchProjectNote: vi.fn(),
  saveUserNote: vi.fn(),
  saveAiNote: vi.fn(),
  generateAiNote: vi.fn(),
  fetchAiHistory: vi.fn(),
  restoreAiNote: vi.fn(),
}));

import * as api from "../../api/projectNoteApi";

const emptyNote = {
  project_id: "p1",
  user_md: "",
  ai_md: null,
  ai_edited: false,
  ai_generated_at: null,
  updated_at: null,
};

describe("projectNoteStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useProjectNoteStore.setState({
      note: null,
      history: [],
      loading: false,
      generating: false,
      error: null,
    });
  });

  it("load はノートをストアへ入れる", async () => {
    vi.mocked(api.fetchProjectNote).mockResolvedValue({
      ...emptyNote,
      user_md: "会場メモ",
    });
    await useProjectNoteStore.getState().load("p1");
    expect(useProjectNoteStore.getState().note?.user_md).toBe("会場メモ");
    expect(useProjectNoteStore.getState().loading).toBe(false);
  });

  it("ノート未作成なら空ノートとして扱う", async () => {
    vi.mocked(api.fetchProjectNote).mockResolvedValue(null);
    await useProjectNoteStore.getState().load("p1");
    expect(useProjectNoteStore.getState().note?.user_md).toBe("");
  });

  it("saveUser は保存後にローカル状態を更新する", async () => {
    vi.mocked(api.fetchProjectNote).mockResolvedValue(emptyNote);
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(api.saveUserNote).mockResolvedValue(undefined);

    await useProjectNoteStore.getState().saveUser("p1", "新しいノート");

    expect(api.saveUserNote).toHaveBeenCalledWith("p1", "新しいノート");
    expect(useProjectNoteStore.getState().note?.user_md).toBe("新しいノート");
  });

  it("saveAi は ai_edited を true にする", async () => {
    vi.mocked(api.fetchProjectNote).mockResolvedValue(emptyNote);
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(api.saveAiNote).mockResolvedValue(undefined);

    await useProjectNoteStore.getState().saveAi("p1", "手で直した");

    expect(useProjectNoteStore.getState().note?.ai_edited).toBe(true);
    expect(useProjectNoteStore.getState().note?.ai_md).toBe("手で直した");
  });

  it("generate は結果を反映し ai_edited をリセットする", async () => {
    vi.mocked(api.fetchProjectNote).mockResolvedValue({
      ...emptyNote,
      ai_md: "旧",
      ai_edited: true,
    });
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(api.generateAiNote).mockResolvedValue({
      ai_md: "新しい要約",
      dropped_mails: 0,
    });

    await useProjectNoteStore.getState().generate("p1", false);

    const s = useProjectNoteStore.getState();
    expect(s.note?.ai_md).toBe("新しい要約");
    expect(s.note?.ai_edited).toBe(false);
    expect(s.generating).toBe(false);
  });

  it("generate 失敗時は error を立て既存 ai_md を保持する", async () => {
    vi.mocked(api.fetchProjectNote).mockResolvedValue({
      ...emptyNote,
      ai_md: "既存の要約",
    });
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(api.generateAiNote).mockRejectedValue(new Error("LLM失敗"));

    await useProjectNoteStore.getState().generate("p1", false);

    const s = useProjectNoteStore.getState();
    expect(s.error).toBeTruthy();
    expect(s.note?.ai_md).toBe("既存の要約");
    expect(s.generating).toBe(false);
  });
});
```

- [ ] **Step 3: テストを実行して失敗を確認**

Run: `pnpm vitest run src/__tests__/stores/projectNoteStore.test.ts`
Expected: FAIL — `Cannot find module '../../stores/projectNoteStore'`

- [ ] **Step 4: ストアを実装**

`src/stores/projectNoteStore.ts`:

```typescript
import { create } from "zustand";
import * as api from "../api/projectNoteApi";
import type { AiHistoryEntry, ProjectNote } from "../types/projectNote";

function emptyNote(projectId: string): ProjectNote {
  return {
    project_id: projectId,
    user_md: "",
    ai_md: null,
    ai_edited: false,
    ai_generated_at: null,
    updated_at: null,
  };
}

interface ProjectNoteState {
  note: ProjectNote | null;
  history: AiHistoryEntry[];
  loading: boolean;
  generating: boolean;
  error: string | null;
  load: (projectId: string) => Promise<void>;
  saveUser: (projectId: string, userMd: string) => Promise<void>;
  saveAi: (projectId: string, aiMd: string) => Promise<void>;
  generate: (projectId: string, cloud: boolean) => Promise<void>;
  loadHistory: (projectId: string) => Promise<void>;
  restore: (projectId: string, historyId: string) => Promise<void>;
}

export const useProjectNoteStore = create<ProjectNoteState>((set, get) => ({
  note: null,
  history: [],
  loading: false,
  generating: false,
  error: null,

  load: async (projectId) => {
    set({ loading: true, error: null });
    try {
      const note = await api.fetchProjectNote(projectId);
      set({ note: note ?? emptyNote(projectId), loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  saveUser: async (projectId, userMd) => {
    try {
      await api.saveUserNote(projectId, userMd);
      const cur = get().note ?? emptyNote(projectId);
      set({ note: { ...cur, user_md: userMd }, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  saveAi: async (projectId, aiMd) => {
    try {
      await api.saveAiNote(projectId, aiMd);
      const cur = get().note ?? emptyNote(projectId);
      set({ note: { ...cur, ai_md: aiMd, ai_edited: true }, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  generate: async (projectId, cloud) => {
    set({ generating: true, error: null });
    try {
      const out = await api.generateAiNote(projectId, cloud);
      const cur = get().note ?? emptyNote(projectId);
      set({
        note: { ...cur, ai_md: out.ai_md, ai_edited: false },
        generating: false,
      });
    } catch (e) {
      // 生成失敗時は既存 ai_md を保持する（設計書 §7）
      set({ error: String(e), generating: false });
    }
  },

  loadHistory: async (projectId) => {
    try {
      const history = await api.fetchAiHistory(projectId);
      set({ history, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  restore: async (projectId, historyId) => {
    try {
      await api.restoreAiNote(historyId);
      await get().load(projectId);
      await get().loadHistory(projectId);
    } catch (e) {
      set({ error: String(e) });
    }
  },
}));
```

- [ ] **Step 5: テストを実行して成功を確認**

Run: `pnpm vitest run src/__tests__/stores/projectNoteStore.test.ts`
Expected: 全6テスト PASS

- [ ] **Step 6: コミット**

```bash
git add src/api/projectNoteApi.ts src/stores/projectNoteStore.ts src/__tests__/stores/projectNoteStore.test.ts
git commit -m "feat(project-notes): 案件ノートのAPIラッパとZustandストアを追加"
```

---

### Task 8: エディタコンポーネント

**Files:**
- Create: `src/components/project-note/ProjectNoteEditor.tsx`
- Create: `src/__tests__/ProjectNoteEditor.test.tsx`

**Interfaces:**
- Consumes: Task 6 の `NOTE_EXTENSIONS`
- Produces: `<ProjectNoteEditor value={string} onChange={(md: string) => void} ariaLabel={string} />`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/ProjectNoteEditor.test.tsx`:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ProjectNoteEditor } from "../components/project-note/ProjectNoteEditor";

describe("ProjectNoteEditor", () => {
  it("初期 Markdown を表示する", () => {
    render(
      <ProjectNoteEditor value="# 春公演" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByLabelText("案件ノート")).toBeInTheDocument();
  });

  it("ツールバーに表の挿入ボタンがある", () => {
    render(
      <ProjectNoteEditor value="" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByLabelText("表を挿入")).toBeInTheDocument();
  });

  it("見出し・太字・箇条書きのボタンがある", () => {
    render(
      <ProjectNoteEditor value="" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByLabelText("見出し")).toBeInTheDocument();
    expect(screen.getByLabelText("太字")).toBeInTheDocument();
    expect(screen.getByLabelText("箇条書き")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認**

Run: `pnpm vitest run src/__tests__/ProjectNoteEditor.test.tsx`
Expected: FAIL — `Cannot find module '../components/project-note/ProjectNoteEditor'`

- [ ] **Step 3: 実装する**

`src/components/project-note/ProjectNoteEditor.tsx`（既存 `RichTextEditor.tsx` のツールバー様式に倣う）:

```typescript
import { useEffect } from "react";
import { useEditor, EditorContent } from "@tiptap/react";
import { NOTE_EXTENSIONS } from "../../utils/markdown";

interface ProjectNoteEditorProps {
  /** 現在の Markdown 本文 */
  value: string;
  /** 編集内容が変わるたびに Markdown を返す */
  onChange: (markdown: string) => void;
  ariaLabel: string;
}

/**
 * 案件ノート用の TipTap エディタ。
 * 保存形式は Markdown（設計書 2026-07-19-project-notes-design.md §3）。
 * 見出し・太字・斜体・箇条書き・番号リスト・リンク・表をサポート（画像は非対応）。
 */
export function ProjectNoteEditor({
  value,
  onChange,
  ariaLabel,
}: ProjectNoteEditorProps) {
  const editor = useEditor({
    extensions: NOTE_EXTENSIONS,
    content: value,
    onUpdate: ({ editor }) => onChange(editor.storage.markdown.getMarkdown()),
    editorProps: {
      attributes: {
        class:
          "prose prose-sm max-w-none min-h-[12rem] rounded border px-2 py-1 focus:outline-none",
        "aria-label": ariaLabel,
      },
    },
  });

  // 外部から value がまるごと差し替わった場合（タブ切替・AI生成後）に同期する
  useEffect(() => {
    if (editor && editor.storage.markdown.getMarkdown() !== value) {
      editor.commands.setContent(value, { emitUpdate: false });
    }
  }, [editor, value]);

  if (!editor) return null;

  const btn = (active: boolean) =>
    `rounded px-2 py-0.5 text-sm hover:bg-gray-100 ${
      active ? "bg-gray-200 font-semibold" : ""
    }`;

  const setLink = () => {
    const prev = editor.getAttributes("link").href as string | undefined;
    const url = window.prompt("リンク先URL", prev ?? "https://");
    if (url === null) return;
    if (url === "") {
      editor.chain().focus().unsetLink().run();
      return;
    }
    editor.chain().focus().extendMarkRange("link").setLink({ href: url }).run();
  };

  return (
    <div className="flex flex-1 flex-col gap-1">
      <div className="flex items-center gap-1 border-b pb-1" role="toolbar">
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
          className={btn(editor.isActive("heading", { level: 2 }))}
          aria-label="見出し"
        >
          H
        </button>
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleBold().run()}
          className={btn(editor.isActive("bold"))}
          aria-label="太字"
        >
          B
        </button>
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleItalic().run()}
          className={`${btn(editor.isActive("italic"))} italic`}
          aria-label="斜体"
        >
          I
        </button>
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleBulletList().run()}
          className={btn(editor.isActive("bulletList"))}
          aria-label="箇条書き"
        >
          •
        </button>
        <button
          type="button"
          onClick={() =>
            editor
              .chain()
              .focus()
              .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
              .run()
          }
          className={btn(editor.isActive("table"))}
          aria-label="表を挿入"
        >
          ⊞
        </button>
        <button
          type="button"
          onClick={setLink}
          className={btn(editor.isActive("link"))}
          aria-label="リンク"
        >
          🔗
        </button>
      </div>
      <EditorContent editor={editor} className="flex-1 overflow-y-auto" />
    </div>
  );
}
```

- [ ] **Step 4: テストを実行して成功を確認**

Run: `pnpm vitest run src/__tests__/ProjectNoteEditor.test.tsx`
Expected: 全3テスト PASS

- [ ] **Step 5: コミット**

```bash
git add src/components/project-note/ProjectNoteEditor.tsx src/__tests__/ProjectNoteEditor.test.tsx
git commit -m "feat(project-notes): 表対応の案件ノートエディタを追加"
```

---

### Task 9: パネル（タブ + 生成 + 確認ダイアログ + 履歴）

**Files:**
- Create: `src/components/project-note/ProjectNotePanel.tsx`
- Create: `src/__tests__/ProjectNotePanel.test.tsx`

**Interfaces:**
- Consumes: Task 7 の `useProjectNoteStore`、Task 8 の `ProjectNoteEditor`
- Produces: `<ProjectNotePanel projectId={string} />`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/ProjectNotePanel.test.tsx`:

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectNotePanel } from "../components/project-note/ProjectNotePanel";
import { useProjectNoteStore } from "../stores/projectNoteStore";

vi.mock("../stores/projectNoteStore");

const baseNote = {
  project_id: "p1",
  user_md: "会場メモ",
  ai_md: "- 公演: 春公演",
  ai_edited: false,
  ai_generated_at: "2026-07-19T10:00:00Z",
  updated_at: null,
};

function mockStore(overrides: Record<string, unknown> = {}) {
  const state = {
    note: baseNote,
    history: [],
    loading: false,
    generating: false,
    error: null,
    load: vi.fn(),
    saveUser: vi.fn(),
    saveAi: vi.fn(),
    generate: vi.fn(),
    loadHistory: vi.fn(),
    restore: vi.fn(),
    ...overrides,
  };
  vi.mocked(useProjectNoteStore).mockReturnValue(state);
  return state;
}

describe("ProjectNotePanel", () => {
  beforeEach(() => vi.clearAllMocks());

  it("ノートタブとAI要約タブを表示する", () => {
    mockStore();
    render(<ProjectNotePanel projectId="p1" />);
    expect(screen.getByRole("tab", { name: "ノート" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "AI要約" })).toBeInTheDocument();
  });

  it("初期表示はノートタブ", () => {
    mockStore();
    render(<ProjectNotePanel projectId="p1" />);
    expect(screen.getByLabelText("案件ノート")).toBeInTheDocument();
  });

  it("AI要約タブへ切り替えられる", async () => {
    mockStore();
    render(<ProjectNotePanel projectId="p1" />);
    await userEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    expect(screen.getByLabelText("AI要約")).toBeInTheDocument();
  });

  it("手修正が無ければ確認なしで生成する", async () => {
    const s = mockStore({ note: { ...baseNote, ai_edited: false } });
    render(<ProjectNotePanel projectId="p1" />);
    await userEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    await userEvent.click(screen.getByRole("button", { name: /再生成/ }));
    await waitFor(() => expect(s.generate).toHaveBeenCalledWith("p1", false));
  });

  it("手修正があれば確認ダイアログを出し、承認するまで生成しない", async () => {
    const s = mockStore({ note: { ...baseNote, ai_edited: true } });
    render(<ProjectNotePanel projectId="p1" />);
    await userEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    await userEvent.click(screen.getByRole("button", { name: /再生成/ }));

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(s.generate).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "上書きする" }));
    await waitFor(() => expect(s.generate).toHaveBeenCalledWith("p1", false));
  });

  it("確認ダイアログをキャンセルすると生成しない", async () => {
    const s = mockStore({ note: { ...baseNote, ai_edited: true } });
    render(<ProjectNotePanel projectId="p1" />);
    await userEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    await userEvent.click(screen.getByRole("button", { name: /再生成/ }));
    await userEvent.click(screen.getByRole("button", { name: "キャンセル" }));

    expect(s.generate).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("生成中はボタンを無効化する", async () => {
    mockStore({ generating: true });
    render(<ProjectNotePanel projectId="p1" />);
    await userEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    expect(screen.getByRole("button", { name: /生成中/ })).toBeDisabled();
  });

  it("エラーを表示する", async () => {
    mockStore({ error: "LLMに接続できません" });
    render(<ProjectNotePanel projectId="p1" />);
    expect(screen.getByText(/LLMに接続できません/)).toBeInTheDocument();
  });

  it("履歴から復元できる", async () => {
    const s = mockStore({
      history: [
        {
          id: "h1",
          project_id: "p1",
          ai_md: "以前の要約",
          replaced_at: "2026-07-18T10:00:00Z",
        },
      ],
    });
    render(<ProjectNotePanel projectId="p1" />);
    await userEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    await userEvent.click(screen.getByRole("button", { name: /履歴/ }));
    await userEvent.click(screen.getByRole("button", { name: "この版に戻す" }));
    await waitFor(() => expect(s.restore).toHaveBeenCalledWith("p1", "h1"));
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認**

Run: `pnpm vitest run src/__tests__/ProjectNotePanel.test.tsx`
Expected: FAIL — `Cannot find module '../components/project-note/ProjectNotePanel'`

- [ ] **Step 3: 実装する**

`src/components/project-note/ProjectNotePanel.tsx`:

```typescript
import { useEffect, useState } from "react";
import { useProjectNoteStore } from "../../stores/projectNoteStore";
import { ProjectNoteEditor } from "./ProjectNoteEditor";

interface ProjectNotePanelProps {
  projectId: string;
}

type Tab = "note" | "ai";

/**
 * 案件ノートのパネル。「ノート」と「AI要約」をタブで切り替える。
 * AI要約もユーザーが編集でき、再生成時は手修正があれば確認ダイアログを出す
 * （設計書 2026-07-19-project-notes-design.md §3-5）。
 */
export function ProjectNotePanel({ projectId }: ProjectNotePanelProps) {
  const {
    note,
    history,
    generating,
    error,
    load,
    saveUser,
    saveAi,
    generate,
    loadHistory,
    restore,
  } = useProjectNoteStore();

  const [tab, setTab] = useState<Tab>("note");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  useEffect(() => {
    void load(projectId);
  }, [projectId, load]);

  const onRegenerate = () => {
    if (note?.ai_edited) {
      setConfirmOpen(true);
      return;
    }
    void generate(projectId, false);
  };

  const confirmRegenerate = () => {
    setConfirmOpen(false);
    void generate(projectId, false);
  };

  const openHistory = () => {
    setHistoryOpen(true);
    void loadHistory(projectId);
  };

  const tabCls = (active: boolean) =>
    `px-3 py-1 text-sm ${active ? "border-b-2 border-blue-500 font-semibold" : ""}`;

  return (
    <div className="flex flex-col gap-2 p-2">
      <div role="tablist" className="flex border-b">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "note"}
          className={tabCls(tab === "note")}
          onClick={() => setTab("note")}
        >
          ノート
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "ai"}
          className={tabCls(tab === "ai")}
          onClick={() => setTab("ai")}
        >
          AI要約
        </button>
      </div>

      {error && (
        <p role="alert" className="text-sm text-red-600">
          {error}
        </p>
      )}

      {tab === "note" && (
        <ProjectNoteEditor
          value={note?.user_md ?? ""}
          onChange={(md) => void saveUser(projectId, md)}
          ariaLabel="案件ノート"
        />
      )}

      {tab === "ai" && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onRegenerate}
              disabled={generating}
              className="rounded border px-2 py-1 text-sm disabled:opacity-50"
            >
              {generating ? "生成中…" : note?.ai_md ? "再生成" : "生成"}
            </button>
            <button
              type="button"
              onClick={openHistory}
              className="rounded border px-2 py-1 text-sm"
            >
              履歴
            </button>
            {note?.ai_generated_at && (
              <span className="text-xs text-gray-500">
                最終生成: {note.ai_generated_at}
              </span>
            )}
          </div>

          <ProjectNoteEditor
            value={note?.ai_md ?? ""}
            onChange={(md) => void saveAi(projectId, md)}
            ariaLabel="AI要約"
          />
        </div>
      )}

      {confirmOpen && (
        <div role="dialog" aria-label="再生成の確認" className="rounded border p-3">
          <p className="text-sm">
            AI要約に手動の修正があります。再生成すると上書きされます（元の内容は履歴から戻せます）。
          </p>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              onClick={confirmRegenerate}
              className="rounded border px-2 py-1 text-sm"
            >
              上書きする
            </button>
            <button
              type="button"
              onClick={() => setConfirmOpen(false)}
              className="rounded border px-2 py-1 text-sm"
            >
              キャンセル
            </button>
          </div>
        </div>
      )}

      {historyOpen && (
        <div className="rounded border p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold">AI要約の履歴</span>
            <button
              type="button"
              onClick={() => setHistoryOpen(false)}
              className="text-sm"
              aria-label="履歴を閉じる"
            >
              ×
            </button>
          </div>
          {history.length === 0 ? (
            <p className="text-sm text-gray-500">履歴はありません</p>
          ) : (
            <ul className="mt-2 flex flex-col gap-2">
              {history.map((h) => (
                <li key={h.id} className="rounded border p-2">
                  <div className="text-xs text-gray-500">{h.replaced_at}</div>
                  <pre className="whitespace-pre-wrap text-xs">{h.ai_md}</pre>
                  <button
                    type="button"
                    onClick={() => void restore(projectId, h.id)}
                    className="mt-1 rounded border px-2 py-0.5 text-xs"
                  >
                    この版に戻す
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: テストを実行して成功を確認**

Run: `pnpm vitest run src/__tests__/ProjectNotePanel.test.tsx`
Expected: 全9テスト PASS

- [ ] **Step 5: コミット**

```bash
git add src/components/project-note/ProjectNotePanel.tsx src/__tests__/ProjectNotePanel.test.tsx
git commit -m "feat(project-notes): タブ・再生成確認・履歴復元のノートパネルを追加"
```

---

### Task 10: 中央ペインへの組み込みと既存同期の付け替え

**Files:**
- Modify: `src/components/thread-list/`（案件選択時の中央ペイン。実装前に該当コンポーネントを特定する）
- Modify: `src-tauri/src/project_context/mod.rs:86-100`（自己修復を DB 正本前提へ）

**Interfaces:**
- Consumes: Task 9 の `ProjectNotePanel`、Task 4 の `project_notes_sync::import_note_from_disk`

- [ ] **Step 1: 組み込み先を特定する**

```bash
grep -rn "selectedProjectId\|selectedProject" src/components/thread-list/ src/App.tsx | head -20
```

案件が選択されているときに中央ペイン上部へ差し込める箇所を特定する。

- [ ] **Step 2: 折りたたみで組み込む**

特定したコンポーネントの、スレッド一覧より上に追加する。`projectId` が null のときは何も描画しない:

```typescript
{selectedProjectId && (
  <details className="border-b">
    <summary className="cursor-pointer px-2 py-1 text-sm font-semibold">
      案件ノート
    </summary>
    <ProjectNotePanel projectId={selectedProjectId} />
  </details>
)}
```

import を追加:

```typescript
import { ProjectNotePanel } from "../project-note/ProjectNotePanel";
```

**注意:** 変数名 `selectedProjectId` は仮。Step 1 で特定した実際の名前を使うこと。

- [ ] **Step 3: 既存の自己修復を DB 正本前提へ付け替える**

`src-tauri/src/project_context/mod.rs:86-100` の「構成不変なら自己修復のみ」ブロックを、`project_contexts` のキャッシュだけ更新する現行から、**`project_notes` へ取り込んでからキャッシュを再生成する**形へ変更する:

```rust
    // --- 4. 構成不変なら自己修復のみ（md外部編集の取り込み） ---
    if prev_inventory_hash.as_deref() == Some(scan.inventory_hash.as_str()) {
        let conn = db.lock().map_err(AppError::lock_err)?;
        // 正本は project_notes。外部エディタでの md 編集を DB へ取り込む
        crate::project_notes_sync::import_note_from_disk(&conn, project_id)?;
        crate::project_notes_sync::refresh_cached_context(&conn, project_id)?;
        return Ok(RescanOutcome {
            status: "ok".to_string(),
            regenerated: false,
            file_count: scan.files.len(),
        });
    }
```

- [ ] **Step 4: 既存テストが壊れていないか確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS

`project_context::mod` の既存テスト（`mod.rs:237`, `:278`, `:335` 付近で `PIGEON-CONTEXT.md` を検証しているもの）が失敗する場合は、DB 正本化に伴う期待値の変化として**テスト側を新しい正しい挙動に合わせて更新**する。挙動が壊れているのかテストが古いのかを必ず判別してから直すこと。

- [ ] **Step 5: フロント全テストと型チェック**

Run: `pnpm vitest run && pnpm tsc --noEmit`
Expected: 全 PASS、型エラーなし

- [ ] **Step 6: Rust の整形とリント**

Run: `cd src-tauri && cargo fmt && cargo clippy -- -D warnings`
Expected: 差分が出たら `cargo fmt` の結果をコミットに含める。clippy 警告ゼロ

**注意:** `cargo fmt` はリポジトリ全体を整形するため、無関係なファイルに差分が出た場合はコミットに含めないこと（`git add` は対象ファイルのみ）。

- [ ] **Step 7: コミット**

```bash
git add src/components src-tauri/src/project_context/mod.rs
git commit -m "feat(project-notes): 中央ペインへノートを組み込み自己修復をDB正本前提に変更"
```

---

### Task 11: 実機確認と設計書のステータス更新

**Files:**
- Modify: `docs/design/2026-07-19-project-notes-design.md`（ステータス行）

- [ ] **Step 1: アプリを起動して目視確認**

Run: `pnpm tauri dev`

以下を実際に確認する（テストが通ることと動くことは別）:

1. **ディレクトリ未連携の案件**を選び「案件ノート」を開く → ノートタブで文字が書けること
2. 太字・見出し・箇条書き・**表の挿入**が動くこと
3. アプリを再起動して**内容が残っている**こと（DB 保存の確認）
4. AI要約タブで「生成」→ Ollama が動いていれば要約が出ること
5. AI要約を手で編集 → 「再生成」で**確認ダイアログ**が出ること
6. 上書き後、「履歴」から**前の版に戻せる**こと
7. **ディレクトリ連携済みの案件**でノートを編集 → 連携フォルダの `PIGEON-CONTEXT.md` に反映されること

- [ ] **Step 2: 見つかった不具合を修正**

不具合があれば、症状を抑えるパッチではなく原因を直す（`agent.md` 不具合修正方針）。修正には必ずテストを先に追加する。

- [ ] **Step 3: 設計書のステータスを更新**

`docs/design/2026-07-19-project-notes-design.md` の先頭を変更:

```markdown
- ステータス: 実装済み（2026-07-19）
```

- [ ] **Step 4: コミット**

```bash
git add docs/design/2026-07-19-project-notes-design.md
git commit -m "docs(project-notes): 設計書のステータスを実装済みに更新"
```

- [ ] **Step 5: PR を作成**

```bash
git push -u origin docs/project-notes-design
gh pr create --title "案件ごとの自由記述ノートをアプリ内で編集できるようにする" --body "$(cat <<'EOF'
## 概要

ディレクトリ連携の有無に関わらず、すべての案件が「案件ノート」を持てるようにした。
これまで案件の自由記述情報は `PIGEON-CONTEXT.md` としてディレクトリ連携時にのみ生成され、
外部エディタでの編集を前提にしていた。ディレクトリを連携しない案件には置き場所が無く、
Markdown ファイルの直接編集は一般ユーザーには馴染みが薄いという課題があった。

## 変更内容

- 案件ノートの正本を SQLite (`project_notes`) に一本化
- アプリ内 TipTap WYSIWYG で編集（見出し・太字・斜体・箇条書き・リンク・表。画像は非対応）
- 「ノート」（手書き）と「AI要約」（案件メールから生成）をタブで分離
- AI要約もユーザーが編集可能。再生成時は手修正があれば確認ダイアログを表示し、旧版は履歴から復元可能
- ディレクトリ連携済み案件では `PIGEON-CONTEXT.md` と双方向同期（正本は DB）
- 分類プロンプト注入 (`cached_context`) は既存経路のまま維持

## 設計書

`docs/design/2026-07-19-project-notes-design.md`

## テスト

- Rust: migration・CRUD・履歴剪定・md 合成分解ラウンドトリップ・メール入力ビルダー（送信境界1000字・件数上限50）
- React: Markdown ラウンドトリップ（表含む）・ストア・エディタ・パネル（確認ダイアログ・履歴復元）
- 実機目視確認済み

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01AY6euhkcbXV6vw1mXGADh8
EOF
)"
```

---

## Self-Review

**1. Spec coverage**

| 設計書の要件 | 対応タスク |
|---|---|
| §3-1 正本は SQLite、`project_contexts` と分離 | Task 1, 2 |
| §3-2 Markdown 保存、TipTap で WYSIWYG、表対応 | Task 6, 8 |
| §3-3 `user_md` / `ai_md` 2区画、UI はタブ | Task 2, 9 |
| §3-4 AI 生成の LLM 境界（1000字・classifier 同等） | Task 3 |
| §3-5 再生成の確認ダイアログ・履歴退避 | Task 2（履歴）, 9（ダイアログ） |
| §3-6 `PIGEON-CONTEXT.md` 双方向同期 | Task 4, 10 |
| §3-7 分類パイプライン無改修 | Task 4 (`refresh_cached_context`) |
| §5 Tauri コマンド6本 | Task 5 |
| §6 コンポーネント配置 | Task 9, 10 |
| §7 エラーハンドリング（生成失敗で ai_md 保持・メール0件） | Task 5, 7 |
| §8 テスト | 各タスクに内包 |
| §10 移行（ファイル→DB 初期化） | Task 4 (`import_note_from_disk`), Task 10 |

全要件にタスクが対応している。

**2. Placeholder scan**

「TBD」「後で実装」等は無し。ただし**実装時に現物確認が必要な箇所**を意図的に残しており、各所に確認手順を明記した:
- migration 番号（並行作業での衝突歴があるため）
- `Mail` 構造体のフィールド名（Task 3）
- `DbState` / LLM State の実際の型とロック取得様式（Task 5）
- invoke 引数の命名規約（Task 7）
- 中央ペインの組み込み先変数名（Task 10）

これらは「詳細を省いた」のではなく「既存コードに合わせる必要がある」箇所で、確認コマンドと合わせ方を具体的に書いてある。

**3. Type consistency**

- `ProjectNote` のフィールド名は Rust（`user_md`, `ai_md`, `ai_edited`, `ai_generated_at`）と TS で一致
- `replace_ai_md_with_history` は `&mut Connection`、`upsert_*` は `&Connection` で一貫
- ストアのメソッド名（`load` / `saveUser` / `saveAi` / `generate` / `loadHistory` / `restore`）はテスト・実装・パネルで一致
- `GenerateNoteOutcome.dropped_mails` は Rust の `usize` → TS の `number` で対応

**未解決リスク（実装時に判明する）:** `tiptap-markdown` の GFM 表ラウンドトリップが期待通り動くかは実際に試すまで確定しない。Task 6 Step 6 で明示的に検証し、通らないまま先へ進まないよう注記済み。ここが破綻した場合は保存形式を HTML に変える設計変更が必要になるため、**Task 6 は Task 8-9 より前に完了させること**（プランの順序はこれを満たしている）。
