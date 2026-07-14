# Phase 4: Search Feature Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SQLite FTS5 による全文検索機能を実装し、メールの件名・本文・送信者を横断的に検索できるようにする。案件を跨いだ検索にも対応する。日本語 2 文字以上の部分一致検索をサポートする。

**Architecture:** DB マイグレーション (v4) で `fts_mails` FTS5 仮想テーブル (`trigram` tokenizer) を作成し、既存メールをバックフィル。`db/search.rs` に検索クエリ関数を集約する。trigram は 3 文字以上で動作するため、2 文字以下のクエリには LIKE フォールバックを使う二段構え。ユーザー入力はサニタイズして FTS5 構文エラーを防ぐ。`commands/search_commands.rs` で Tauri command として公開。フロントエンドは `searchStore.ts` で状態管理し、サイドバーの検索バーから検索 → 中央ペインに結果表示する。スニペット表示は DOMPurify で `<b>` タグのみ許可してサニタイズする。検索結果クリック時は `selectThread(null)` で既存スレッド選択をクリアしてから `selectMail` を呼ぶ。

**Tech Stack:** Rust (rusqlite FTS5 trigram + LIKE fallback), Tauri commands, React + Zustand + TypeScript, Tailwind CSS, DOMPurify

---

## File Structure

### Rust (バックエンド)

| ファイル | 責務 |
|---------|------|
| `src-tauri/src/db/migrations.rs` | 修正: v4 マイグレーション追加 (fts_mails テーブル trigram tokenizer + トリガー + バックフィル) |
| `src-tauri/src/db/search.rs` | 新規: FTS5 検索クエリ関数 + LIKE フォールバック + ユーザー入力サニタイズ |
| `src-tauri/src/db/mod.rs` | 修正: `pub mod search;` 追加 |
| `src-tauri/src/models/mail.rs` | 修正: `SearchResult` 構造体追加 |
| `src-tauri/src/commands/search_commands.rs` | 新規: `search_mails` Tauri command |
| `src-tauri/src/commands/mod.rs` | 修正: `pub mod search_commands;` 追加 |
| `src-tauri/src/lib.rs` | 修正: `search_mails` をハンドラに登録 |

### React (フロントエンド)

| ファイル | 責務 |
|---------|------|
| `src/types/mail.ts` | 修正: `SearchResult` 型追加 |
| `src/stores/searchStore.ts` | 新規: 検索状態管理 (query, results, loading) |
| `src/stores/uiStore.ts` | 修正: ViewMode に `"search"` 追加 |
| `src/components/sidebar/SearchBar.tsx` | 新規: 検索入力コンポーネント |
| `src/components/sidebar/Sidebar.tsx` | 修正: SearchBar を組み込む |
| `src/components/thread-list/SearchResults.tsx` | 新規: 検索結果一覧表示 (DOMPurify + selectThread(null)) |
| `src/components/mail-view/MailView.tsx` | 修正: `selectedMail` 単独でも表示できるよう対応 |
| `src/App.tsx` | 修正: `viewMode === "search"` のルーティング追加 |

---

## Task 1: FTS5 マイグレーション (v4) — trigram tokenizer

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`

- [ ] **Step 1: マイグレーション v4 のテストを書く**

`src-tauri/src/db/migrations.rs` の `#[cfg(test)] mod tests` ブロックの末尾に追加:

```rust
#[test]
fn test_v4_migration_creates_fts_table() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    // Verify fts_mails virtual table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='fts_mails'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(table_exists, "fts_mails table should exist after v4 migration");

    // Schema version should be 4
    let version: i32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

#[test]
fn test_v4_migration_backfills_existing_mails() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

    // Bootstrap schema_version table via get_schema_version (creates table + inserts row)
    get_schema_version(&conn).unwrap();

    // Manually run v1-v3 migrations without FTS
    migrate_v1(&conn).unwrap();
    set_schema_version(&conn, 1).unwrap();
    migrate_v2(&conn).unwrap();
    set_schema_version(&conn, 2).unwrap();
    migrate_v3(&conn).unwrap();
    set_schema_version(&conn, 3).unwrap();

    // Insert data while no FTS triggers exist
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
         VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'sender@example.com', 'me@example.com', 'BackfillTest subject', 'body text here', '2026-04-13', 1)",
        [],
    ).unwrap();

    // Verify no FTS table exists yet
    let fts_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='fts_mails'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!fts_exists, "fts_mails should not exist before v4");

    // Now run full migrations — v4 should backfill the existing mail into FTS
    run_migrations(&conn).unwrap();

    let fts_count: i32 = conn
        .query_row("SELECT COUNT(*) FROM fts_mails", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fts_count, 1, "backfill should populate fts_mails for pre-existing mails");

    // Verify the backfilled content is searchable
    let search_count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"BackfillTest\"'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(search_count, 1, "backfilled mail should be searchable");
}

#[test]
fn test_v4_fts_trigger_on_insert() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
         VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'alice@example.com', 'me@example.com', 'Meeting Tomorrow', 'Let us discuss the project plan', '2026-04-13', 1)",
        [],
    ).unwrap();

    // trigram tokenizer: substring match with 3+ chars
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"Meeting\"'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_v4_fts_trigger_on_delete() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
         VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'alice@example.com', 'me@example.com', 'DeleteTarget', 'body', '2026-04-13', 1)",
        [],
    ).unwrap();

    conn.execute("DELETE FROM mails WHERE id = 'm1'", []).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"DeleteTarget\"'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "FTS entry should be removed when mail is deleted");
}

#[test]
fn test_v4_fts_japanese_3char_search() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
         VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
         VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'sender@example.com', 'me@example.com', '見積もりの件', '予算について相談があります', '2026-04-13', 1)",
        [],
    ).unwrap();

    // trigram: 3+ char Japanese substring works via FTS
    let subject_count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"見積もり\"'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(subject_count, 1, "3+ char Japanese substring search should work via FTS trigram");
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

Run: `cd src-tauri && cargo test test_v4 -- --nocapture`
Expected: FAIL — `fts_mails` テーブルが存在しない

- [ ] **Step 3: v4 マイグレーションを実装**

`src-tauri/src/db/migrations.rs` に `migrate_v4` 関数を追加し、`run_migrations` に組み込む。

**重要:** `trigram` tokenizer を使う。これにより 3 文字以上の任意の文字列（日本語含む）の部分一致検索が可能。2 文字以下のクエリは検索関数 (Task 2) で LIKE フォールバックする。

```rust
fn migrate_v4(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS fts_mails USING fts5(
            mail_id UNINDEXED,
            subject,
            body_text,
            from_addr,
            to_addr,
            tokenize = 'trigram'
        );

        -- Auto-sync FTS on INSERT (INSERT OR REPLACE triggers DELETE then INSERT)
        CREATE TRIGGER IF NOT EXISTS trg_fts_mails_insert
        AFTER INSERT ON mails
        BEGIN
            INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
            VALUES (NEW.id, NEW.subject, COALESCE(NEW.body_text, ''), NEW.from_addr, NEW.to_addr);
        END;

        -- Auto-sync FTS on DELETE
        CREATE TRIGGER IF NOT EXISTS trg_fts_mails_delete
        AFTER DELETE ON mails
        BEGIN
            DELETE FROM fts_mails WHERE mail_id = OLD.id;
        END;

        -- Backfill existing mails into FTS
        INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
        SELECT id, subject, COALESCE(body_text, ''), from_addr, to_addr
        FROM mails
        WHERE id NOT IN (SELECT mail_id FROM fts_mails);
        ",
    )?;
    Ok(())
}
```

`run_migrations` 関数の末尾 (`let _ = version;` の前) に追加:

```rust
if version < 4 {
    migrate_v4(conn)?;
    version = 4;
    set_schema_version(conn, version)?;
}
```

- [ ] **Step 4: テストを実行して全て通ることを確認**

Run: `cd src-tauri && cargo test test_v4 -- --nocapture`
Expected: 5 tests PASS

- [ ] **Step 5: 既存テストが壊れていないことを確認**

Run: `cd src-tauri && cargo test`
Expected: ALL tests PASS

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/db/migrations.rs
git commit -m "feat(db): add v4 migration for FTS5 full-text search with trigram tokenizer

Create fts_mails virtual table using trigram tokenizer for Japanese
substring search (3+ chars). Auto-sync triggers on INSERT/DELETE.
Backfill existing mails into FTS index."
```

---

## Task 2: 検索クエリ関数 (`db/search.rs`) — FTS5 + LIKE 二段構え

**Files:**
- Create: `src-tauri/src/db/search.rs`
- Modify: `src-tauri/src/db/mod.rs`
- Modify: `src-tauri/src/models/mail.rs`

- [ ] **Step 1: SearchResult 構造体を追加**

`src-tauri/src/models/mail.rs` の末尾に追加:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub mail: Mail,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub snippet: String,
}
```

- [ ] **Step 2: `db/mod.rs` に search モジュールを追加**

`src-tauri/src/db/mod.rs` に追加:

```rust
pub mod search;
```

- [ ] **Step 3: 検索関数のテストを書く**

`src-tauri/src/db/search.rs` を新規作成:

```rust
use crate::db::mails::{row_to_mail, MAIL_COLUMNS_PREFIXED};
use crate::error::AppError;
use crate::models::mail::SearchResult;
use rusqlite::{params, Connection};

/// Sanitize user input for FTS5 trigram queries.
/// Wraps the input in double quotes to treat it as a literal substring match,
/// escaping any internal double quotes.
fn sanitize_fts_query(query: &str) -> String {
    let escaped = query.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

/// Check if a query is long enough for FTS5 trigram (>= 3 characters).
/// Trigram tokenizer creates 3-character tokens, so shorter queries
/// cannot match and must use LIKE fallback.
fn is_fts_eligible(query: &str) -> bool {
    query.chars().count() >= 3
}

/// Search mails using FTS5 trigram for queries >= 3 chars,
/// or LIKE fallback for shorter queries (e.g. 2-char Japanese words).
/// `account_id` scopes the search to a single account.
/// Returns up to `limit` results.
pub fn search_mails(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{assignments, mails, projects};
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{make_mail, setup_db};

    // --- sanitize_fts_query tests ---

    #[test]
    fn test_sanitize_plain_text() {
        assert_eq!(sanitize_fts_query("hello"), "\"hello\"");
    }

    #[test]
    fn test_sanitize_with_special_chars() {
        assert_eq!(sanitize_fts_query("foo-bar"), "\"foo-bar\"");
        assert_eq!(sanitize_fts_query("user@example.com"), "\"user@example.com\"");
    }

    #[test]
    fn test_sanitize_with_double_quotes() {
        assert_eq!(sanitize_fts_query("say \"hello\""), "\"say \"\"hello\"\"\"");
    }

    #[test]
    fn test_sanitize_japanese() {
        assert_eq!(sanitize_fts_query("見積もり"), "\"見積もり\"");
    }

    // --- is_fts_eligible tests ---

    #[test]
    fn test_fts_eligible_3_chars() {
        assert!(is_fts_eligible("abc"));
        assert!(is_fts_eligible("見積も"));
    }

    #[test]
    fn test_fts_eligible_2_chars() {
        assert!(!is_fts_eligible("ab"));
        assert!(!is_fts_eligible("予算"));
    }

    #[test]
    fn test_fts_eligible_1_char() {
        assert!(!is_fts_eligible("a"));
        assert!(!is_fts_eligible("予"));
    }

    // --- escape_like tests ---

    #[test]
    fn test_escape_like_plain() {
        assert_eq!(escape_like("hello"), "hello");
    }

    #[test]
    fn test_escape_like_percent() {
        assert_eq!(escape_like("100%"), "100\\%");
    }

    #[test]
    fn test_escape_like_underscore() {
        assert_eq!(escape_like("a_b"), "a\\_b");
    }

    #[test]
    fn test_escape_like_backslash() {
        assert_eq!(escape_like("a\\b"), "a\\\\b");
    }

    // --- search_mails tests ---

    #[test]
    fn test_search_by_subject_3plus_chars() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "見積もりの件", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "議事録の共有", "2026-04-13T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        // 4 chars — uses FTS trigram
        let results = search_mails(&conn, "acc1", "見積もり", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_by_subject_2char_japanese() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Subject", "2026-04-13T10:00:00");
        m1.body_text = Some("プロジェクトの予算について相談があります".into());
        mails::insert_mail(&conn, &m1).unwrap();

        // 2 chars — uses LIKE fallback
        let results = search_mails(&conn, "acc1", "予算", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_by_from_addr() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        m1.from_addr = "tanaka@example.com".into();
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "tanaka", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "zzzznonexistent", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        let mut m1 = make_mail("m1", "<msg1@ex.com>", "SharedKeyword", "2026-04-13T10:00:00");
        m1.account_id = "acc1".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "SharedKeyword", "2026-04-13T11:00:00");
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        let results = search_mails(&conn, "acc1", "SharedKeyword", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_search_includes_project_info() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "AlphaMail subject", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project Alpha".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();

        let results = search_mails(&conn, "acc1", "AlphaMail", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project_id, Some(proj.id));
        assert_eq!(results[0].project_name, Some("Project Alpha".into()));
    }

    #[test]
    fn test_search_unclassified_mail_has_no_project() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "OrphanMail text", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "OrphanMail", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].project_id.is_none());
        assert!(results[0].project_name.is_none());
    }

    #[test]
    fn test_search_respects_limit() {
        let conn = setup_db();
        for i in 0..10 {
            let m = make_mail(
                &format!("m{}", i),
                &format!("<msg{}@ex.com>", i),
                &format!("CommonKeyword item{}", i),
                &format!("2026-04-13T1{}:00:00", i),
            );
            mails::insert_mail(&conn, &m).unwrap();
        }

        let results = search_mails(&conn, "acc1", "CommonKeyword", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_snippet_not_empty_fts() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Report", "2026-04-13T10:00:00");
        m1.body_text = Some("The quarterly revenue report shows growth in Q1".into());
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "revenue", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].snippet.is_empty());
    }

    #[test]
    fn test_search_empty_query_returns_empty() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_with_special_chars_no_error() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "foo-bar baz", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        // These should NOT cause FTS5 syntax errors
        let results = search_mails(&conn, "acc1", "foo-bar", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_with_fts_operators_safely_handled() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello world", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        // FTS5 operators like AND, OR, NOT should be treated as literals
        let results = search_mails(&conn, "acc1", "AND OR NOT", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_2char_like_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        let mut m1 = make_mail("m1", "<msg1@ex.com>", "予算の件", "2026-04-13T10:00:00");
        m1.account_id = "acc1".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "予算計画", "2026-04-13T11:00:00");
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        // 2 chars → LIKE fallback, should still be scoped to account
        let results = search_mails(&conn, "acc1", "予算", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_search_2char_like_snippet() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "件名", "2026-04-13T10:00:00");
        m1.body_text = Some("本文の内容".into());
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "件名", 50).unwrap();
        assert_eq!(results.len(), 1);
        // LIKE fallback snippet should use subject as fallback
        assert!(!results[0].snippet.is_empty());
    }
}
```

- [ ] **Step 4: テストを実行して失敗を確認**

Run: `cd src-tauri && cargo test db::search -- --nocapture`
Expected: FAIL — `todo!()` でパニック

- [ ] **Step 5: search_mails 関数を実装**

`search_mails` の `todo!()` を以下に置き換え:

```rust
pub fn search_mails(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if is_fts_eligible(trimmed) {
        search_fts(conn, account_id, trimmed, limit)
    } else {
        search_like(conn, account_id, trimmed, limit)
    }
}

/// FTS5 trigram search for queries with 3+ characters.
fn search_fts(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let safe_query = sanitize_fts_query(query);

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name,
                snippet(fts_mails, 1, '<b>', '</b>', '...', 32) AS snip
         FROM fts_mails fts
         JOIN mails m ON fts.mail_id = m.id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE fts_mails MATCH ?1
           AND m.account_id = ?2
         ORDER BY rank
         LIMIT ?3",
        MAIL_COLUMNS_PREFIXED
    ))?;

    let results = stmt
        .query_map(params![safe_query, account_id, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(18)?;
            let project_name: Option<String> = row.get(19)?;
            let snippet: String = row.get(20)?;
            Ok(SearchResult {
                mail,
                project_id,
                project_name,
                snippet,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Escape LIKE special characters (`%`, `_`, `\`) so user input is
/// treated as a literal substring. Uses `\` as the ESCAPE character.
fn escape_like(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len());
    for ch in query.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// LIKE fallback for queries with < 3 characters (e.g. 2-char Japanese words).
/// No FTS5 ranking or snippet available, so we use subject as snippet.
fn search_like(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let like_pattern = format!("%{}%", escape_like(query));

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name
         FROM mails m
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE m.account_id = ?1
           AND (m.subject LIKE ?2 ESCAPE '\\' OR m.body_text LIKE ?2 ESCAPE '\\' OR m.from_addr LIKE ?2 ESCAPE '\\' OR m.to_addr LIKE ?2 ESCAPE '\\')
         ORDER BY m.date DESC
         LIMIT ?3",
        MAIL_COLUMNS_PREFIXED
    ))?;

    let results = stmt
        .query_map(params![account_id, like_pattern, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(18)?;
            let project_name: Option<String> = row.get(19)?;
            Ok(SearchResult {
                mail: mail.clone(),
                project_id,
                project_name,
                snippet: mail.subject,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}
```

- [ ] **Step 6: テストを実行して全て通ることを確認**

Run: `cd src-tauri && cargo test db::search -- --nocapture`
Expected: ALL tests PASS

- [ ] **Step 7: 全テストが通ることを確認**

Run: `cd src-tauri && cargo test`
Expected: ALL tests PASS

- [ ] **Step 8: コミット**

```bash
git add src-tauri/src/db/search.rs src-tauri/src/db/mod.rs src-tauri/src/models/mail.rs
git commit -m "feat(search): add FTS5 search with LIKE fallback for short queries

FTS5 trigram for 3+ char queries with ranking and snippets.
LIKE fallback for 1-2 char queries (e.g. 2-char Japanese words).
User input is sanitized to prevent FTS5 syntax errors."
```

---

## Task 3: Tauri search command

**Files:**
- Create: `src-tauri/src/commands/search_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: search_commands.rs を作成**

```rust
use tauri::State;

use crate::db::search;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::state::DbState;

#[tauri::command]
pub fn search_mails(
    state: State<DbState>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    search::search_mails(&conn, &account_id, &query, 100)
}
```

- [ ] **Step 2: commands/mod.rs に search_commands を追加**

`src-tauri/src/commands/mod.rs` に追加:

```rust
pub mod search_commands;
```

- [ ] **Step 3: lib.rs のハンドラに search_mails を登録**

`src-tauri/src/lib.rs` の `invoke_handler` 内の最後の行 (`commands::classify_commands::get_mails_by_project,` の後) に追加:

```rust
commands::search_commands::search_mails,
```

- [ ] **Step 4: ビルドが通ることを確認**

Run: `cd src-tauri && cargo build`
Expected: BUILD SUCCESS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/commands/search_commands.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(search): add search_mails Tauri command

Exposes FTS5 search to frontend via invoke('search_mails')."
```

---

## Task 4: フロントエンド型定義 + 検索ストア + DOMPurify

**Files:**
- Modify: `src/types/mail.ts`
- Create: `src/stores/searchStore.ts`
- Modify: `src/stores/uiStore.ts`

- [ ] **Step 1: DOMPurify をインストール**

Run: `pnpm add dompurify && pnpm add -D @types/dompurify`

- [ ] **Step 2: SearchResult 型を追加**

`src/types/mail.ts` の末尾に追加:

```typescript
export interface SearchResult {
  mail: Mail;
  project_id: string | null;
  project_name: string | null;
  snippet: string;
}
```

- [ ] **Step 3: uiStore に search ビューモードを追加**

`src/stores/uiStore.ts` の `ViewMode` 型を修正:

```typescript
export type ViewMode = "threads" | "unclassified" | "project" | "search";
```

- [ ] **Step 4: searchStore.ts を作成**

```typescript
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult } from "../types/mail";
import { useErrorStore } from "./errorStore";

interface SearchState {
  query: string;
  results: SearchResult[];
  searching: boolean;
  search: (accountId: string, query: string) => Promise<void>;
  clearSearch: () => void;
}

export const useSearchStore = create<SearchState>((set) => ({
  query: "",
  results: [],
  searching: false,

  search: async (accountId, query) => {
    if (!query.trim()) {
      set({ query: "", results: [], searching: false });
      return;
    }
    set({ query, searching: true });
    try {
      const results = await invoke<SearchResult[]>("search_mails", {
        accountId,
        query,
      });
      set({ results, searching: false });
    } catch (e) {
      set({ results: [], searching: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  clearSearch: () => set({ query: "", results: [], searching: false }),
}));
```

- [ ] **Step 5: TypeScript の型チェックが通ることを確認**

Run: `pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 6: コミット**

```bash
git add src/types/mail.ts src/stores/searchStore.ts src/stores/uiStore.ts package.json pnpm-lock.yaml
git commit -m "feat(search): add SearchResult type, searchStore, DOMPurify, and search ViewMode

Frontend state management for search. DOMPurify added for safe
snippet rendering."
```

---

## Task 5: SearchBar コンポーネント

**Files:**
- Create: `src/components/sidebar/SearchBar.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`

- [ ] **Step 1: SearchBar コンポーネントのテストを書く**

`src/__tests__/SearchBar.test.tsx` を新規作成:

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SearchBar } from "../components/sidebar/SearchBar";

describe("SearchBar", () => {
  const mockOnSearch = vi.fn();
  const mockOnClear = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders search input", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    expect(screen.getByPlaceholderText("検索...")).toBeInTheDocument();
  });

  it("calls onSearch when Enter is pressed", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    const input = screen.getByPlaceholderText("検索...");
    fireEvent.change(input, { target: { value: "test query" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(mockOnSearch).toHaveBeenCalledWith("test query");
  });

  it("calls onClear when Escape is pressed", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    const input = screen.getByPlaceholderText("検索...");
    fireEvent.change(input, { target: { value: "something" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(mockOnClear).toHaveBeenCalled();
  });

  it("does not call onSearch for empty query", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    const input = screen.getByPlaceholderText("検索...");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(mockOnSearch).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認**

Run: `pnpm vitest run src/__tests__/SearchBar.test.tsx`
Expected: FAIL — モジュールが見つからない

- [ ] **Step 3: SearchBar コンポーネントを実装**

`src/components/sidebar/SearchBar.tsx` を新規作成。最初から `forwardRef` 対応にしておく:

```typescript
import { useState, useRef, forwardRef, useImperativeHandle } from "react";

interface SearchBarProps {
  onSearch: (query: string) => void;
  onClear: () => void;
}

export interface SearchBarHandle {
  focus: () => void;
}

export const SearchBar = forwardRef<SearchBarHandle, SearchBarProps>(
  function SearchBar({ onSearch, onClear }, ref) {
    const [value, setValue] = useState("");
    const inputRef = useRef<HTMLInputElement>(null);

    useImperativeHandle(ref, () => ({
      focus: () => inputRef.current?.focus(),
    }));

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && value.trim()) {
        onSearch(value.trim());
      } else if (e.key === "Escape") {
        setValue("");
        onClear();
        inputRef.current?.blur();
      }
    };

    return (
      <div className="px-3 py-2">
        <input
          ref={inputRef}
          type="text"
          placeholder="検索..."
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          className="w-full rounded border border-gray-300 bg-white px-3 py-1.5 text-sm focus:border-blue-400 focus:outline-none"
        />
      </div>
    );
  }
);
```

- [ ] **Step 4: テストを実行して全て通ることを確認**

Run: `pnpm vitest run src/__tests__/SearchBar.test.tsx`
Expected: ALL tests PASS

- [ ] **Step 5: Sidebar に SearchBar を組み込む**

`src/components/sidebar/Sidebar.tsx` を修正:

ファイル冒頭のインポートを修正:
```typescript
import { useEffect, useState, useRef } from "react";
```

インポートに追加:
```typescript
import { SearchBar } from "./SearchBar";
import type { SearchBarHandle } from "./SearchBar";
import { useSearchStore } from "../../stores/searchStore";
```

コンポーネント内にストアの取得と ref と keyboard handler を追加:
```typescript
const { search, clearSearch } = useSearchStore();
const searchBarRef = useRef<SearchBarHandle>(null);

const handleSearch = (query: string) => {
  if (!selectedAccountId) return;
  search(selectedAccountId, query);
  setViewMode("search");
};

const handleClearSearch = () => {
  clearSearch();
  setViewMode("threads");
};

useEffect(() => {
  const handleKeyDown = (e: KeyboardEvent) => {
    if (
      e.target instanceof HTMLInputElement ||
      e.target instanceof HTMLTextAreaElement
    ) {
      return;
    }
    if (e.key === "/") {
      e.preventDefault();
      searchBarRef.current?.focus();
    }
  };
  window.addEventListener("keydown", handleKeyDown);
  return () => window.removeEventListener("keydown", handleKeyDown);
}, []);
```

JSX 内、`{showForm && ...}` ブロックの直後、`<div className="flex-1 overflow-y-auto">` の直前に追加:
```tsx
<SearchBar ref={searchBarRef} onSearch={handleSearch} onClear={handleClearSearch} />
```

- [ ] **Step 6: 型チェック通過を確認**

Run: `pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 7: コミット**

```bash
git add src/components/sidebar/SearchBar.tsx src/__tests__/SearchBar.test.tsx src/components/sidebar/Sidebar.tsx
git commit -m "feat(ui): add SearchBar component with '/' keyboard shortcut

Enter to search, Escape to clear. '/' focuses search bar.
forwardRef for parent focus control."
```

---

## Task 6: MailView の検索結果対応

**Files:**
- Modify: `src/components/mail-view/MailView.tsx`

検索結果をクリックしたとき、`selectedThread` がなくても `selectedMail` 単独でメールを表示できるようにする。

- [ ] **Step 1: MailView を修正**

`src/components/mail-view/MailView.tsx` を修正:

変更前:
```typescript
export function MailView() {
  const { selectedThread, selectedMail, selectMail } = useMailStore();

  if (!selectedThread) {
    return <EmptyState message="スレッドを選択してください" />;
  }

  const mail =
    selectedMail ?? selectedThread.mails[selectedThread.mails.length - 1];

  return (
    <div className="flex h-full flex-col">
      <MailTabs
        mails={selectedThread.mails}
        activeMailId={mail.id}
        onSelect={selectMail}
      />
      <MailHeader mail={mail} />
      <MailBody mail={mail} />
    </div>
  );
}
```

変更後:
```typescript
export function MailView() {
  const { selectedThread, selectedMail, selectMail } = useMailStore();

  if (!selectedThread && !selectedMail) {
    return <EmptyState message="スレッドを選択してください" />;
  }

  // Search result mode: selectedMail without a thread — skip MailTabs
  if (!selectedThread && selectedMail) {
    return (
      <div className="flex h-full flex-col">
        <MailHeader mail={selectedMail} />
        <MailBody mail={selectedMail} />
      </div>
    );
  }

  const mail =
    selectedMail ?? selectedThread!.mails[selectedThread!.mails.length - 1];

  return (
    <div className="flex h-full flex-col">
      <MailTabs
        mails={selectedThread!.mails}
        activeMailId={mail.id}
        onSelect={selectMail}
      />
      <MailHeader mail={mail} />
      <MailBody mail={mail} />
    </div>
  );
}
```

- [ ] **Step 2: 型チェック通過を確認**

Run: `pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 3: 既存のテストが壊れていないことを確認**

Run: `pnpm vitest run`
Expected: ALL tests PASS

- [ ] **Step 4: コミット**

```bash
git add src/components/mail-view/MailView.tsx
git commit -m "fix(ui): support MailView with selectedMail only (no thread)

When a search result is clicked, selectedMail is set without
selectedThread. MailView now renders the mail directly in this case,
skipping the thread tabs."
```

---

## Task 7: SearchResults コンポーネント — DOMPurify + selectThread(null)

**Files:**
- Create: `src/components/thread-list/SearchResults.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: SearchResults コンポーネントのテストを書く**

`src/__tests__/SearchResults.test.tsx` を新規作成:

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock searchStore
const mockSearchStore = {
  query: "",
  results: [] as import("../types/mail").SearchResult[],
  searching: false,
};
vi.mock("../stores/searchStore", () => ({
  useSearchStore: (selector: (s: typeof mockSearchStore) => unknown) =>
    selector(mockSearchStore),
}));

// Mock mailStore — track calls to selectThread and selectMail
const mockSelectThread = vi.fn();
const mockSelectMail = vi.fn();
vi.mock("../stores/mailStore", () => ({
  useMailStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({
      selectThread: mockSelectThread,
      selectMail: mockSelectMail,
    }),
}));

import { SearchResults } from "../components/thread-list/SearchResults";
import type { SearchResult, Mail } from "../types/mail";

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1",
    account_id: "acc1",
    folder: "INBOX",
    message_id: "<msg1@ex.com>",
    in_reply_to: null,
    references: null,
    from_addr: "sender@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject: "Test Subject",
    body_text: "Test body",
    body_html: null,
    date: "2026-04-13T10:00:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    fetched_at: "2026-04-13T00:00:00",
    ...overrides,
  };
}

describe("SearchResults", () => {
  beforeEach(() => {
    mockSearchStore.query = "";
    mockSearchStore.results = [];
    mockSearchStore.searching = false;
    vi.clearAllMocks();
  });

  it("shows loading state", () => {
    mockSearchStore.searching = true;
    mockSearchStore.query = "test";
    render(<SearchResults />);
    expect(screen.getByText("検索中...")).toBeInTheDocument();
  });

  it("shows empty state when no results", () => {
    mockSearchStore.query = "nonexistent";
    mockSearchStore.results = [];
    render(<SearchResults />);
    expect(screen.getByText(/検索結果がありません/)).toBeInTheDocument();
  });

  it("renders search results", () => {
    const result: SearchResult = {
      mail: makeMail({ subject: "見積もりの件" }),
      project_id: "proj1",
      project_name: "案件A",
      snippet: "...<b>見積もり</b>について...",
    };
    mockSearchStore.query = "見積もり";
    mockSearchStore.results = [result];
    render(<SearchResults />);
    expect(screen.getByText("見積もりの件")).toBeInTheDocument();
    expect(screen.getByText("案件A")).toBeInTheDocument();
  });

  it("shows unclassified label when no project", () => {
    const result: SearchResult = {
      mail: makeMail({ subject: "Orphan" }),
      project_id: null,
      project_name: null,
      snippet: "...",
    };
    mockSearchStore.query = "orphan";
    mockSearchStore.results = [result];
    render(<SearchResults />);
    expect(screen.getByText("未分類")).toBeInTheDocument();
  });

  it("sanitizes dangerous HTML in snippets", () => {
    const result: SearchResult = {
      mail: makeMail({ subject: "XSS test" }),
      project_id: null,
      project_name: null,
      snippet: '<b>safe</b><script>alert("xss")</script>',
    };
    mockSearchStore.query = "xss";
    mockSearchStore.results = [result];
    const { container } = render(<SearchResults />);
    // <script> should be stripped by DOMPurify
    expect(container.querySelector("script")).toBeNull();
    // <b> should be preserved
    expect(container.querySelector("b")?.textContent).toBe("safe");
  });

  it("clears selectedThread and sets selectedMail on click", () => {
    const mail = makeMail({ subject: "Click Me" });
    const result: SearchResult = {
      mail,
      project_id: null,
      project_name: null,
      snippet: "...",
    };
    mockSearchStore.query = "click";
    mockSearchStore.results = [result];
    render(<SearchResults />);

    fireEvent.click(screen.getByText("Click Me"));

    // Must clear thread first to prevent MailView from showing stale MailTabs
    expect(mockSelectThread).toHaveBeenCalledWith(null);
    expect(mockSelectMail).toHaveBeenCalledWith(mail);
    // selectThread(null) must be called before selectMail
    const threadCallOrder = mockSelectThread.mock.invocationCallOrder[0];
    const mailCallOrder = mockSelectMail.mock.invocationCallOrder[0];
    expect(threadCallOrder).toBeLessThan(mailCallOrder);
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認**

Run: `pnpm vitest run src/__tests__/SearchResults.test.tsx`
Expected: FAIL — モジュールが見つからない

- [ ] **Step 3: SearchResults コンポーネントを実装**

`src/components/thread-list/SearchResults.tsx` を新規作成:

```typescript
import DOMPurify from "dompurify";
import { useSearchStore } from "../../stores/searchStore";
import { useMailStore } from "../../stores/mailStore";
import { EmptyState } from "../common/EmptyState";
import type { SearchResult } from "../../types/mail";

/** Sanitize FTS5 snippet HTML, allowing only <b> tags for highlights. */
function sanitizeSnippet(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS: ["b"] });
}

function SearchResultItem({
  result,
  onClick,
}: {
  result: SearchResult;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="w-full border-b px-4 py-3 text-left hover:bg-gray-50"
    >
      <div className="flex items-center justify-between">
        <span className="truncate text-sm font-medium">
          {result.mail.subject}
        </span>
        <span className="ml-2 shrink-0 text-xs text-gray-400">
          {result.mail.date.slice(0, 10)}
        </span>
      </div>
      <div className="mt-1 flex items-center gap-2">
        <span className="truncate text-xs text-gray-500">
          {result.mail.from_addr}
        </span>
        <span
          className={`shrink-0 rounded px-1.5 py-0.5 text-xs ${
            result.project_name
              ? "bg-blue-100 text-blue-700"
              : "bg-gray-100 text-gray-500"
          }`}
        >
          {result.project_name ?? "未分類"}
        </span>
      </div>
      <p
        className="mt-1 truncate text-xs text-gray-400"
        dangerouslySetInnerHTML={{ __html: sanitizeSnippet(result.snippet) }}
      />
    </button>
  );
}

export function SearchResults() {
  const query = useSearchStore((s) => s.query);
  const results = useSearchStore((s) => s.results);
  const searching = useSearchStore((s) => s.searching);
  const selectThread = useMailStore((s) => s.selectThread);
  const selectMail = useMailStore((s) => s.selectMail);

  const handleResultClick = (result: SearchResult) => {
    // Clear any existing thread selection first to prevent MailView
    // from rendering stale MailTabs
    selectThread(null);
    selectMail(result.mail);
  };

  if (searching) {
    return <EmptyState message="検索中..." />;
  }

  if (query && results.length === 0) {
    return <EmptyState message={`「${query}」の検索結果がありません`} />;
  }

  if (!query) {
    return <EmptyState message="キーワードを入力して検索" />;
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="border-b bg-gray-50 px-4 py-2 text-xs text-gray-500">
        「{query}」の検索結果: {results.length}件
      </div>
      {results.map((result) => (
        <SearchResultItem
          key={result.mail.id}
          result={result}
          onClick={() => handleResultClick(result)}
        />
      ))}
    </div>
  );
}
```

- [ ] **Step 4: テストを実行して全て通ることを確認**

Run: `pnpm vitest run src/__tests__/SearchResults.test.tsx`
Expected: ALL tests PASS

- [ ] **Step 5: App.tsx に search ビューモードのルーティングを追加**

`src/App.tsx` の冒頭のインポートに追加:
```typescript
import { SearchResults } from "./components/thread-list/SearchResults";
```

`<div className="w-80 border-r">` 内の条件分岐を修正:

変更前:
```tsx
{viewMode === "unclassified" ? (
  <UnclassifiedList />
) : (
  <ThreadList viewMode={viewMode} />
)}
```

変更後:
```tsx
{viewMode === "search" ? (
  <SearchResults />
) : viewMode === "unclassified" ? (
  <UnclassifiedList />
) : (
  <ThreadList viewMode={viewMode} />
)}
```

- [ ] **Step 6: 型チェック通過を確認**

Run: `pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 7: 全フロントエンドテストが通ることを確認**

Run: `pnpm vitest run`
Expected: ALL tests PASS

- [ ] **Step 8: コミット**

```bash
git add src/components/thread-list/SearchResults.tsx src/__tests__/SearchResults.test.tsx src/App.tsx
git commit -m "feat(ui): add SearchResults with DOMPurify and thread clearing

Displays FTS5 search results with project badge and sanitized snippet.
Only <b> tags allowed in snippet HTML (XSS prevention).
Clicks call selectThread(null) then selectMail to prevent stale MailTabs."
```

---

## Task 8: 動作確認と全テスト実行

**Files:** (変更なし)

- [ ] **Step 1: Rust 全テスト実行**

Run: `cd src-tauri && cargo test`
Expected: ALL tests PASS

- [ ] **Step 2: フロントエンド全テスト実行**

Run: `pnpm vitest run`
Expected: ALL tests PASS

- [ ] **Step 3: ビルドチェック**

Run: `cd src-tauri && cargo build`
Expected: BUILD SUCCESS

- [ ] **Step 4: dev サーバーで手動動作確認**

Run: `pnpm tauri dev`

確認事項:
1. サイドバーに検索バーが表示されること
2. 検索バーにキーワードを入力して Enter → 中央ペインに検索結果が表示されること
3. 3 文字以上の日本語 (例: 「見積もり」) → FTS5 で検索結果が返ること
4. 2 文字の日本語 (例: 「予算」) → LIKE フォールバックで検索結果が返ること
5. 検索結果に案件名バッジが表示されること (分類済みメール)
6. 検索結果に「未分類」バッジが表示されること (未分類メール)
7. 検索結果をクリック → 右ペインにメール本文が表示されること (**MailTabs なし**)
8. スレッドを選択した後に検索結果をクリック → 右ペインが検索結果のメールに切り替わること (**MailTabs なし**、前のスレッドのタブが残らない)
9. Escape キー → 検索クリア、通常ビューに戻ること
10. `/` キー → 検索バーにフォーカスが移ること
11. 特殊文字を含むクエリ (例: `foo-bar`, `user@example.com`) でエラーにならないこと
