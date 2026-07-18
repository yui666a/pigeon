# 案件階層化 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 案件（projects）を深さ無制限の木構造にし、集約表示・階層対応AI分類・加算的継承・サブツリー限定検索を提供する。

**Architecture:** 隣接リスト（`parent_id` 自己参照FK）+ 再帰CTE。階層不変条件（循環・同一アカウント・account_id不変）はDBトリガーが最終防衛線。削除は葉先行の明示削除（FK CASCADE の再帰上限回避）。correction_log はパススナップショット化して案件削除後も few-shot を保存。階層変更操作は全て UseCase 化して ADR-0004 dispatch バス経由。

**Tech Stack:** Rust / rusqlite 0.31 (bundled SQLite 3.45, トリガー内再帰CTE可) / Tauri 2 / React 19 + Zustand 5 / Vitest

**設計書:** `docs/design/2026-07-18-hierarchical-projects-design.md`（承認済み。Codex 3回レビュー・SQLite実機検証反映済み）

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。エラーは `crate::error::AppError`（検証エラーは `AppError::Validation`、存在なしは `AppError::ProjectNotFound`）
- TDD: 各タスクは失敗するテストを先に書く（Red → Green）
- コミットは Conventional Commits、1コミット=1意図。PRタイトル・本文に内部フェーズ名（「PR①」等）を使わない
- マイグレーションは **v19**（実装時点で他ブランチに v19 が現れていたら次の空き番号に読み替え）。`MIGRATIONS` 配列末尾に追記
- `cargo fmt` は自分が触ったファイルだけをコミットに含める
- テスト実行: `cd src-tauri && cargo test` / フロントは `pnpm test`
- パス文字列の区切りは `" > "`（スペース+大なり+スペース）で全箇所統一
- ユーザー向けエラーメッセージは日本語、トリガーの RAISE メッセージは英語（既存流儀）

## PR 構成（Stacked）

| PR | ブランチ | 内容 | タスク |
|---|---|---|---|
| A | `feat/project-hierarchy-db`（base: main） | v19・階層CRUD・削除/アーカイブ/マージ・スナップショット・集約表示バックエンド | Task 1〜7 |
| B | `feat/project-hierarchy-usecases`（base: A） | 操作系 UseCase 化 + dispatch 移行 + set_project_parent | Task 8 |
| C | `feat/project-hierarchy-classifier`（base: B） | パス付きプロンプト・create with parent | Task 9〜10 |
| D | `feat/project-hierarchy-search`（base: B） | FTS/セマンティックのサブツリースコープ | Task 11 |
| E | `feat/project-hierarchy-ui`（base: B〜D マージ後の main または D） | ツリーUI・集約表示・検索トグル・状態整合 | Task 12〜14 |

## ファイル構成

| ファイル | 役割 |
|---|---|
| Modify: `src-tauri/src/db/migrations.rs` | migrate_v19（parent_id・トリガー5本・correction_log再構築）・FK検査 |
| Modify: `src-tauri/src/models/project.rs` | `Project.parent_id` / `CreateProjectRequest.parent_id` |
| Modify: `src-tauri/src/db/projects.rs` | subtree_ids / ancestor_path / project_path_string / set_parent / 葉先行削除 / サブツリーアーカイブ / マージ検証強化 / build_effective_context |
| Modify: `src-tauri/src/db/assignments.rs` | insert_correction / reassign_with_correction / get_recent_corrections のパススナップショット化・IN句版 get_mails_by_projects |
| Modify: `src-tauri/src/models/classifier.rs` | CorrectionEntry（from_path/to_path）・ProjectSummary.path・Create.parent_project_id |
| Modify: `src-tauri/src/models/mail.rs` | `ThreadProjectRef` / `Thread.projects` |
| Modify: `src-tauri/src/db/mails.rs` | get_threads_by_project のサブツリー集約 |
| Modify: `src-tauri/src/threading.rs` | Thread 構築時の projects 初期化 |
| Create: `src-tauri/src/usecase/cases/project.rs` | 案件構造変更の UseCase 6種 |
| Modify: `src-tauri/src/commands/project_commands.rs` | dispatch 経由への書き換え + set_project_parent |
| Modify: `src-tauri/src/classifier/prompt.rs` | パス付き案件列挙・最深ノード指示・few-shot パス表記 |
| Modify: `src-tauri/src/classifier/service.rs` | create の parent 検証 |
| Modify: `src-tauri/src/commands/classify_commands.rs` | approve_new_project の parent 対応 |
| Modify: `src-tauri/src/db/search.rs` / `src-tauri/src/db/vec_search.rs` | project スコープフィルタ |
| Modify: `src-tauri/src/usecase/cases/search.rs` / `src-tauri/src/commands/search_commands.rs` | スコープ引数の配線 |
| Modify: `src/types/project.ts` / `src/types/mail.ts` | parent_id / Thread.projects |
| Modify: `src/api/projectApi.ts` / `src/api/searchApi.ts` | setProjectParent / スコープ引数 |
| Modify: `src/stores/projectStore.ts` | ツリー構築・集約未読・状態整合契約 |
| Modify: `src/components/sidebar/ProjectTree.tsx` ほか sidebar | 再帰ツリー描画・展開状態・メニュー |
| Create: `src/components/sidebar/MoveProjectDialog.tsx` | 「親を変更...」ツリーピッカー |
| Modify: `src/components/thread-list/ThreadList.tsx` ほか | パンくず・所属チップ・検索トグル |

---

## Task 1: migration v19（parent_id・トリガー・correction_log 再構築）

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`

**Interfaces:**
- Produces: スキーマ v19（projects.parent_id、トリガー5本、correction_log の from_path/to_path 列と SET NULL FK）。後続全タスクの前提
- 注意: `run_migrations` に `PRAGMA foreign_keys` 検査を追加するため、raw `Connection::open_in_memory()` を使う既存テストは冒頭で `conn.execute_batch("PRAGMA foreign_keys=ON;")` が必要になる（`test_helpers::setup_db` は有効化済み）

- [ ] **Step 1: 失敗するテストを書く**

`migrations.rs` のテストモジュールに追加（既存の「最新バージョン」アサーション（`schema_version == 18` としている箇所、約4箇所）を 19 へ更新するのもこのタスク）:

```rust
#[test]
fn test_v19_projects_parent_fk_exists() {
    let conn = crate::test_helpers::setup_db();
    // 自己参照FKが宣言されている
    let fk_table: String = conn
        .query_row(
            "SELECT \"table\" FROM pragma_foreign_key_list('projects') WHERE \"from\" = 'parent_id'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fk_table, "projects");
}

#[test]
fn test_v19_cycle_triggers_reject() {
    let conn = crate::test_helpers::setup_db();
    conn.execute_batch(
        "INSERT INTO projects (id, account_id, name) VALUES ('a', 'acc1', 'A');
         INSERT INTO projects (id, account_id, name) VALUES ('b', 'acc1', 'B');
         UPDATE projects SET parent_id = 'a' WHERE id = 'b';",
    )
    .unwrap();
    // 自分を親に
    let err = conn
        .execute("UPDATE projects SET parent_id = 'a' WHERE id = 'a'", [])
        .unwrap_err();
    assert!(err.to_string().contains("project hierarchy cycle"), "{err}");
    // 子孫を親に
    let err = conn
        .execute("UPDATE projects SET parent_id = 'b' WHERE id = 'a'", [])
        .unwrap_err();
    assert!(err.to_string().contains("project hierarchy cycle"), "{err}");
    // INSERT の自己参照
    let err = conn
        .execute(
            "INSERT INTO projects (id, account_id, name, parent_id) VALUES ('c', 'acc1', 'C', 'c')",
            [],
        )
        .unwrap_err();
    assert!(err.to_string().contains("project hierarchy cycle"), "{err}");
}

#[test]
fn test_v19_parent_account_and_immutability_triggers() {
    let conn = crate::test_helpers::setup_db();
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc2', 'Other', 'o@ex.com', 'imap.ex.com', 'smtp.ex.com', 'plain', 'other')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, account_id, name) VALUES ('a1p', 'acc1', 'P1');
         INSERT INTO projects (id, account_id, name) VALUES ('a2p', 'acc2', 'P2');",
    )
    .unwrap();
    // 異アカウントの親
    let err = conn
        .execute("UPDATE projects SET parent_id = 'a1p' WHERE id = 'a2p'", [])
        .unwrap_err();
    assert!(err.to_string().contains("parent project not found in same account"), "{err}");
    // account_id の更新
    let err = conn
        .execute("UPDATE projects SET account_id = 'acc2' WHERE id = 'a1p'", [])
        .unwrap_err();
    assert!(err.to_string().contains("account_id is immutable"), "{err}");
}

#[test]
fn test_v19_correction_log_has_path_columns_and_set_null_fk() {
    let conn = crate::test_helpers::setup_db();
    let cols: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM pragma_table_info('correction_log')")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    };
    assert!(cols.contains(&"from_path".to_string()));
    assert!(cols.contains(&"to_path".to_string()));
    // to_project の FK が SET NULL になっている
    let on_delete: String = conn
        .query_row(
            "SELECT on_delete FROM pragma_foreign_key_list('correction_log') WHERE \"from\" = 'to_project'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(on_delete, "SET NULL");
}

#[test]
fn test_v19_correction_log_rebuild_preserves_rows_and_sequence() {
    // v18 状態の DB を作り、correction_log に行を入れてから v19 を適用する
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    crate::db::vec_ext::register();
    // v18 までを適用（apply_migrations を部分適用できる既存テストヘルパの流儀に従う。
    // 無ければ MIGRATIONS[..17]（v18 まで）のスライスで apply_migrations を呼ぶ）
    apply_migrations(&conn, &MIGRATIONS[..MIGRATIONS.len() - 1]).unwrap();
    // テストデータ: acc1 は setup_db 相当を手で入れる
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
         VALUES ('acc1', 'T', 't@ex.com', 'i', 's', 'plain', 'other')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', '照明');
         INSERT INTO projects (id, account_id, name) VALUES ('p2', 'acc1', '音響');",
    )
    .unwrap();
    let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "S", "2026-07-18T10:00:00");
    crate::db::mails::insert_mail(&conn, &m).unwrap();
    conn.execute_batch(
        "INSERT INTO correction_log (mail_id, from_project, to_project) VALUES ('m1', 'p1', 'p2');
         INSERT INTO correction_log (mail_id, from_project, to_project) VALUES ('m1', NULL, 'p2');
         DELETE FROM correction_log WHERE id = 2;",
    )
    .unwrap();
    // v19 適用
    apply_migrations(&conn, MIGRATIONS).unwrap();
    // 行が保全され、名前がスナップショットに焼かれている
    let (from_path, to_path): (Option<String>, String) = conn
        .query_row(
            "SELECT from_path, to_path FROM correction_log WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(from_path.as_deref(), Some("照明"));
    assert_eq!(to_path, "音響");
    // AUTOINCREMENT 高水位: 発行済み最大 id=2 が引き継がれ、次の挿入は 3
    conn.execute(
        "INSERT INTO correction_log (mail_id, from_project, from_path, to_project, to_path)
         VALUES ('m1', 'p1', '照明', 'p2', '音響')",
        [],
    )
    .unwrap();
    let new_id: i64 = conn
        .query_row("SELECT MAX(id) FROM correction_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(new_id, 3, "削除済み id=2 が再利用されないこと");
}

#[test]
fn test_run_migrations_requires_foreign_keys_on() {
    crate::db::vec_ext::register();
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    let err = run_migrations(&conn).unwrap_err();
    assert!(err.to_string().contains("foreign_keys"), "{err}");
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::migrations`
Expected: FAIL（migrate_v19 未定義でコンパイルエラー→スタブ追加後、トリガー・列不在で assert 失敗）

- [ ] **Step 3: 実装**

`migrations.rs` に追加し、`MIGRATIONS` 末尾へ `(19, migrate_v19),`:

```rust
/// v19: 案件の階層化。
/// - projects.parent_id（自己参照FK。CASCADE は防御層で、削除の正常経路は
///   db::projects::delete_project の葉先行明示削除——SQLite の FK CASCADE は
///   トリガー再帰深度上限（既定1000）に服するため）
/// - 階層不変条件のトリガー（循環禁止・同一アカウント・account_id 不変）。
///   アプリ層検証の迂回経路（修復スクリプト等）に対する最終防衛線
/// - correction_log をパススナップショット化（from_path/to_path）し FK を両方
///   SET NULL に再構築。マージ・削除で few-shot 学習例が変質・消滅する既存バグの根治
fn migrate_v19(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        r#"
        ALTER TABLE projects ADD COLUMN parent_id TEXT REFERENCES projects(id) ON DELETE CASCADE;
        CREATE INDEX IF NOT EXISTS idx_projects_parent ON projects(parent_id);

        CREATE TRIGGER trg_projects_no_cycle
        BEFORE UPDATE OF parent_id ON projects
        WHEN NEW.parent_id IS NOT NULL
        BEGIN
            SELECT RAISE(ABORT, 'project hierarchy cycle')
            WHERE NEW.parent_id = NEW.id
               OR NEW.parent_id IN (
                    WITH RECURSIVE desc_ids(id) AS (
                        SELECT id FROM projects WHERE parent_id = NEW.id
                        UNION ALL
                        SELECT p.id FROM projects p JOIN desc_ids d ON p.parent_id = d.id
                    )
                    SELECT id FROM desc_ids
               );
        END;

        CREATE TRIGGER trg_projects_no_cycle_insert
        BEFORE INSERT ON projects
        WHEN NEW.parent_id IS NOT NULL AND NEW.parent_id = NEW.id
        BEGIN
            SELECT RAISE(ABORT, 'project hierarchy cycle');
        END;

        CREATE TRIGGER trg_projects_parent_account_insert
        BEFORE INSERT ON projects
        WHEN NEW.parent_id IS NOT NULL
        BEGIN
            SELECT RAISE(ABORT, 'parent project not found in same account')
            WHERE NOT EXISTS (
                SELECT 1 FROM projects pp
                WHERE pp.id = NEW.parent_id AND pp.account_id = NEW.account_id
            );
        END;

        CREATE TRIGGER trg_projects_parent_account_update
        BEFORE UPDATE OF parent_id ON projects
        WHEN NEW.parent_id IS NOT NULL
        BEGIN
            SELECT RAISE(ABORT, 'parent project not found in same account')
            WHERE NOT EXISTS (
                SELECT 1 FROM projects pp
                WHERE pp.id = NEW.parent_id AND pp.account_id = NEW.account_id
            );
        END;

        CREATE TRIGGER trg_projects_account_immutable
        BEFORE UPDATE OF account_id ON projects
        WHEN NEW.account_id != OLD.account_id
        BEGIN
            SELECT RAISE(ABORT, 'project account_id is immutable');
        END;

        CREATE TABLE correction_log_new (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            mail_id      TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
            from_project TEXT REFERENCES projects(id) ON DELETE SET NULL,
            from_path    TEXT,
            to_project   TEXT REFERENCES projects(id) ON DELETE SET NULL,
            to_path      TEXT NOT NULL,
            corrected_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        INSERT INTO correction_log_new (id, mail_id, from_project, from_path, to_project, to_path, corrected_at)
        SELECT cl.id, cl.mail_id, cl.from_project, fp.name, cl.to_project, COALESCE(tp.name, ''), cl.corrected_at
        FROM correction_log cl
        LEFT JOIN projects fp ON cl.from_project = fp.id
        LEFT JOIN projects tp ON cl.to_project = tp.id;
        UPDATE sqlite_sequence
        SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'correction_log')
        WHERE name = 'correction_log_new'
          AND (SELECT seq FROM sqlite_sequence WHERE name = 'correction_log') > seq;
        DROP TABLE correction_log;
        ALTER TABLE correction_log_new RENAME TO correction_log;
        "#,
    )?;
    Ok(())
}
```

`run_migrations` に FK 検査を追加:

```rust
pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    crate::db::vec_ext::register();
    // FK 未強制の接続でスキーマだけ進む事故を防ぐ（トリガー・CASCADE が全て前提にする）
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk != 1 {
        return Err(AppError::Validation(
            "PRAGMA foreign_keys must be ON before running migrations".into(),
        ));
    }
    apply_migrations(conn, MIGRATIONS)
}
```

注意: migrations.rs 内の既存テストで raw `Connection::open_in_memory()` から `run_migrations` を呼ぶものは冒頭に `conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();` を追加する。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test db::migrations && cargo test`
Expected: 全 PASS（v17/v18 からのアップグレードパステスト含む）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/migrations.rs
git commit -m "feat(db): 案件階層化のスキーマを追加(v19)——parent_id・階層不変条件トリガー・correction_logのパススナップショット化"
```

---

## Task 2: Project モデルと階層クエリ関数

**Files:**
- Modify: `src-tauri/src/models/project.rs`
- Modify: `src-tauri/src/db/projects.rs`

**Interfaces:**
- Produces:
  - `Project.parent_id: Option<String>` / `CreateProjectRequest.parent_id: Option<String>`
  - `pub fn subtree_ids(conn, id: &str) -> Result<Vec<String>, AppError>` — 自分+全子孫、**深い順（depth DESC）**。存在しない id は空 Vec
  - `pub fn ancestor_path(conn, id: &str) -> Result<Vec<Project>, AppError>` — ルート→自ノード順
  - `pub fn project_path_string(conn, id: &str) -> Result<String, AppError>` — `"ツアー > 会場 > 音響"`
  - `pub fn set_parent(conn, id: &str, new_parent: Option<&str>) -> Result<(), AppError>`
  - `insert_project_with_id` に `parent_id: Option<&str>` 引数追加（アーカイブ済み親はアプリ層で拒否）

- [ ] **Step 1: 失敗するテストを書く**

`db/projects.rs` のテストに追加:

```rust
fn insert_child(conn: &Connection, id: &str, name: &str, parent: Option<&str>) -> Project {
    insert_project_with_id(conn, id, "acc1", name, None, None, parent).unwrap()
}

#[test]
fn test_subtree_ids_returns_deepest_first() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    insert_child(&conn, "leaf", "音響", Some("mid"));
    insert_child(&conn, "other", "別件", None);

    let ids = subtree_ids(&conn, "root").unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids.last().unwrap(), "root", "自分が最後（最浅）");
    assert!(ids.iter().position(|i| i == "leaf") < ids.iter().position(|i| i == "mid"));
    assert!(subtree_ids(&conn, "nonexistent").unwrap().is_empty());
}

#[test]
fn test_ancestor_path_and_path_string() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    insert_child(&conn, "leaf", "音響", Some("mid"));

    let path = ancestor_path(&conn, "leaf").unwrap();
    let names: Vec<&str> = path.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["ツアー", "埼玉", "音響"]);
    assert_eq!(project_path_string(&conn, "leaf").unwrap(), "ツアー > 埼玉 > 音響");
}

#[test]
fn test_set_parent_validations() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    insert_child(&conn, "arch", "旧公演", None);
    archive_project(&conn, "arch").unwrap();

    // 正常系: ルート化と付け替え
    set_parent(&conn, "mid", None).unwrap();
    set_parent(&conn, "mid", Some("root")).unwrap();
    // 自分自身・子孫は拒否（アプリ層エラー）
    assert!(set_parent(&conn, "root", Some("root")).is_err());
    assert!(set_parent(&conn, "root", Some("mid")).is_err());
    // アーカイブ済み親は拒否
    assert!(set_parent(&conn, "mid", Some("arch")).is_err());
}

#[test]
fn test_create_project_under_archived_parent_is_rejected() {
    let conn = setup_db();
    insert_child(&conn, "arch", "旧公演", None);
    archive_project(&conn, "arch").unwrap();
    let result = insert_project_with_id(&conn, "c1", "acc1", "子", None, None, Some("arch"));
    assert!(result.is_err());
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::projects`
Expected: FAIL（コンパイルエラー: 関数・引数未定義）

- [ ] **Step 3: 実装**

`models/project.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub is_archived: bool,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}
```

`db/projects.rs` — `row_to_project` と全 SELECT 句に `parent_id` を追加（SELECT 列順は
`id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at` に統一。
`row_to_project` の get 添字も追従）。

```rust
pub fn insert_project_with_id(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    description: Option<&str>,
    color: Option<&str>,
    parent_id: Option<&str>,
) -> Result<Project, AppError> {
    if let Some(pid) = parent_id {
        let parent = get_project(conn, pid)?;
        if parent.is_archived {
            return Err(AppError::Validation(
                "アーカイブ済みの案件の下には作成できません".into(),
            ));
        }
    }
    conn.execute(
        "INSERT INTO projects (id, account_id, name, description, color, parent_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, account_id, name, description, color, parent_id],
    )?;
    get_project(conn, id)
}

pub fn insert_project(conn: &Connection, req: &CreateProjectRequest) -> Result<Project, AppError> {
    let id = Uuid::new_v4().to_string();
    insert_project_with_id(
        conn,
        &id,
        &req.account_id,
        &req.name,
        req.description.as_deref(),
        req.color.as_deref(),
        req.parent_id.as_deref(),
    )
}

/// 自分+全子孫の ID を深い順（depth DESC）で返す。
/// 深い順は delete_project の葉先行削除の前提（FK CASCADE の再帰上限を踏まない）。
/// 存在しない id は空 Vec（呼び出し側で ProjectNotFound にする）。
pub fn subtree_ids(conn: &Connection, id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(id, depth) AS (
             SELECT id, 0 FROM projects WHERE id = ?1
             UNION ALL
             SELECT p.id, s.depth + 1 FROM projects p JOIN subtree s ON p.parent_id = s.id
         )
         SELECT id FROM subtree ORDER BY depth DESC, id",
    )?;
    let ids = stmt
        .query_map(params![id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// ルート→自ノード順の祖先パス（自分を含む）。
pub fn ancestor_path(conn: &Connection, id: &str) -> Result<Vec<Project>, AppError> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE anc(id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at, depth) AS (
             SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at, 0
             FROM projects WHERE id = ?1
             UNION ALL
             SELECT p.id, p.account_id, p.name, p.description, p.color, p.is_archived, p.parent_id, p.created_at, p.updated_at, a.depth + 1
             FROM projects p JOIN anc a ON p.id = a.parent_id
         )
         SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at
         FROM anc ORDER BY depth DESC",
    )?;
    let projects = stmt
        .query_map(params![id], row_to_project)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(projects)
}

/// 「ツアー > 会場 > 音響」形式のパス文字列（パンくず・LLM・訂正ログスナップショット用）。
pub fn project_path_string(conn: &Connection, id: &str) -> Result<String, AppError> {
    let path = ancestor_path(conn, id)?;
    if path.is_empty() {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(path.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" > "))
}

/// 親の付け替え。DB トリガーが最終防衛線だが、ユーザー向けの日本語エラーは
/// ここで返す（存在・同一アカウント・循環・アーカイブ済み親）。
pub fn set_parent(conn: &Connection, id: &str, new_parent: Option<&str>) -> Result<(), AppError> {
    let project = get_project(conn, id)?;
    if let Some(parent_id) = new_parent {
        let parent = get_project(conn, parent_id)?;
        if parent.account_id != project.account_id {
            return Err(AppError::Validation(
                "親案件は同じアカウントに属している必要があります".into(),
            ));
        }
        if parent.is_archived {
            return Err(AppError::Validation(
                "アーカイブ済みの案件の下には移動できません".into(),
            ));
        }
        if subtree_ids(conn, id)?.contains(&parent_id.to_string()) {
            return Err(AppError::Validation(
                "自分自身または配下の案件を親にはできません".into(),
            ));
        }
    }
    conn.execute(
        "UPDATE projects SET parent_id = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        params![new_parent, id],
    )?;
    Ok(())
}
```

既存呼び出しの追従: `insert_project_with_id` の既存呼び出し（classify_commands の approve_new_project 等）は末尾引数に `None` を追加。既存テストの `CreateProjectRequest` リテラルには `parent_id: None` を追加（`#[serde(default)]` は JSON 経由のみに効く）。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/project.rs src-tauri/src/db/projects.rs src-tauri/src/commands/classify_commands.rs
git commit -m "feat(db): 案件の階層クエリ（subtree/ancestor/path）と親付け替え・親付き作成を追加"
```

---

## Task 3: 葉先行削除とサブツリーアーカイブ

**Files:**
- Modify: `src-tauri/src/db/projects.rs`

**Interfaces:**
- Consumes: `subtree_ids`（Task 2、深い順契約）
- Produces:
  - `delete_project(conn, id)` — サブツリーを**葉先行**で1トランザクション明示削除
  - `archive_project(conn, id)` — サブツリー一括アーカイブ
  - `pub fn count_subtree_mails(conn, id) -> Result<u32, AppError>` — 削除確認ダイアログ用

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test]
fn test_delete_project_removes_subtree_and_unassigns_mails() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "S", "2026-07-18T10:00:00");
    crate::db::mails::insert_mail(&conn, &m).unwrap();
    assignments::assign_mail(&conn, "m1", "mid", "user", None).unwrap();

    delete_project(&conn, "root").unwrap();

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 0);
    let assigned: i64 = conn
        .query_row("SELECT COUNT(*) FROM mail_project_assignments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(assigned, 0, "メールは未分類に戻る");
}

#[test]
fn test_delete_deep_subtree_leaf_first_avoids_cascade_limit() {
    // FK CASCADE の再帰上限（既定1000）を踏まないことの回帰テスト
    let conn = setup_db();
    conn.execute(
        "WITH RECURSIVE nums(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM nums WHERE n < 1100)
         INSERT INTO projects (id, account_id, name, parent_id)
         SELECT 'd' || n, 'acc1', 'N' || n, CASE WHEN n = 1 THEN NULL ELSE 'd' || (n - 1) END
         FROM nums",
        [],
    )
    .unwrap();
    delete_project(&conn, "d1").unwrap();
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 0);
}

#[test]
fn test_archive_project_archives_subtree() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    archive_project(&conn, "root").unwrap();
    let active: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects WHERE is_archived = FALSE", [], |r| r.get(0))
        .unwrap();
    assert_eq!(active, 0, "親だけ消えて子が宙に浮く状態を作らない");
}

#[test]
fn test_count_subtree_mails() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    for (mid, pid) in [("m1", "root"), ("m2", "mid")] {
        let m = crate::test_helpers::make_mail(mid, &format!("<{mid}@ex>"), "S", "2026-07-18T10:00:00");
        crate::db::mails::insert_mail(&conn, &m).unwrap();
        assignments::assign_mail(&conn, mid, pid, "user", None).unwrap();
    }
    assert_eq!(count_subtree_mails(&conn, "root").unwrap(), 2);
    assert_eq!(count_subtree_mails(&conn, "mid").unwrap(), 1);
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::projects`
Expected: FAIL（deep subtree テストは現行 `DELETE FROM projects WHERE id=?` 単発のため subtree が残って件数不一致）

- [ ] **Step 3: 実装**

```rust
/// サブツリーを葉先行で明示削除する。FK CASCADE（防御層）に深い再帰をさせない。
pub fn delete_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let ids = subtree_ids(conn, id)?; // 深い順 = 葉先行
    if ids.is_empty() {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("DELETE FROM projects WHERE id = ?1")?;
        for pid in &ids {
            stmt.execute(params![pid])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// サブツリー一括アーカイブ（親だけ消えて子が宙に浮く状態を作らない）。
pub fn archive_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let ids = subtree_ids(conn, id)?;
    if ids.is_empty() {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "UPDATE projects SET is_archived = TRUE, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        )?;
        for pid in &ids {
            stmt.execute(params![pid])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// サブツリー配下の所属メール数（削除確認ダイアログ用）。
pub fn count_subtree_mails(conn: &Connection, id: &str) -> Result<u32, AppError> {
    let count: u32 = conn.query_row(
        "WITH RECURSIVE subtree(id) AS (
             SELECT id FROM projects WHERE id = ?1
             UNION ALL
             SELECT p.id FROM projects p JOIN subtree s ON p.parent_id = s.id
         )
         SELECT COUNT(*) FROM mail_project_assignments WHERE project_id IN (SELECT id FROM subtree)",
        params![id],
        |r| r.get(0),
    )?;
    Ok(count)
}
```

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test db::projects && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/projects.rs
git commit -m "feat(db): 案件削除・アーカイブをサブツリー単位にし葉先行削除でCASCADE再帰上限を回避"
```

---

## Task 4: correction_log 消費コードのパススナップショット化

**Files:**
- Modify: `src-tauri/src/models/classifier.rs`（CorrectionEntry）
- Modify: `src-tauri/src/db/assignments.rs`（insert_correction / reassign_with_correction / get_recent_corrections）
- Modify: `src-tauri/src/classifier/prompt.rs`（few-shot 行の生成）

**Interfaces:**
- Consumes: `projects::project_path_string`（Task 2）
- Produces:
  - `CorrectionEntry { mail_subject: String, from_path: Option<String>, to_path: String }`
  - `insert_correction(conn, mail_id, from_project: Option<&str>, from_path: Option<&str>, to_project: &str, to_path: &str)`
  - `reassign_with_correction(conn, mail_id, from_project, to_project)` — シグネチャ不変。内部でパスを解決してスナップショットを渡す
  - `get_recent_corrections(conn, account_id, limit) -> Vec<CorrectionEntry>` — **projects への JOIN を廃止**し from_path/to_path を直接 SELECT（参照先案件が削除された行も返る——これが v19 の主目的）

- [ ] **Step 1: 失敗するテストを書く**

`db/assignments.rs` のテストに追加:

```rust
#[test]
fn test_corrections_survive_project_deletion() {
    let conn = setup_db();
    let from = crate::db::projects::insert_project_with_id(&conn, "pf", "acc1", "照明", None, None, None).unwrap();
    let to = crate::db::projects::insert_project_with_id(&conn, "pt", "acc1", "音響", None, None, None).unwrap();
    let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "仕込み図", "2026-07-18T10:00:00");
    crate::db::mails::insert_mail(&conn, &m).unwrap();
    assign_mail(&conn, "m1", &from.id, "ai", Some(0.9)).unwrap();

    reassign_with_correction(&conn, "m1", &from.id, &to.id).unwrap();
    // from も to も消しても few-shot は生き残る
    crate::db::projects::delete_project(&conn, &to.id).unwrap();
    crate::db::projects::delete_project(&conn, &from.id).unwrap();

    let corrections = get_recent_corrections(&conn, "acc1", 10).unwrap();
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].from_path.as_deref(), Some("照明"));
    assert_eq!(corrections[0].to_path, "音響");
}

#[test]
fn test_correction_snapshot_records_full_path() {
    let conn = setup_db();
    crate::db::projects::insert_project_with_id(&conn, "root", "acc1", "ツアー", None, None, None).unwrap();
    crate::db::projects::insert_project_with_id(&conn, "leaf", "acc1", "音響", None, None, Some("root")).unwrap();
    crate::db::projects::insert_project_with_id(&conn, "leaf2", "acc1", "照明", None, None, Some("root")).unwrap();
    let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "S", "2026-07-18T10:00:00");
    crate::db::mails::insert_mail(&conn, &m).unwrap();
    assign_mail(&conn, "m1", "leaf", "ai", Some(0.9)).unwrap();

    reassign_with_correction(&conn, "m1", "leaf", "leaf2").unwrap();

    let c = &get_recent_corrections(&conn, "acc1", 1).unwrap()[0];
    assert_eq!(c.from_path.as_deref(), Some("ツアー > 音響"));
    assert_eq!(c.to_path, "ツアー > 照明");
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::assignments`
Expected: FAIL（コンパイルエラー: CorrectionEntry フィールド・シグネチャ不一致）

- [ ] **Step 3: 実装**

`models/classifier.rs` の `CorrectionEntry`:

```rust
#[derive(Debug, Clone)]
pub struct CorrectionEntry {
    pub mail_subject: String,
    /// 訂正時点のパススナップショット。None = 未分類からの移動
    pub from_path: Option<String>,
    pub to_path: String,
}
```

`db/assignments.rs`:

```rust
/// 訂正を correction_log に記録する。from_path/to_path は訂正時点のパス
/// スナップショット——案件が後で削除・改名されても few-shot の意味を保存する。
pub fn insert_correction(
    conn: &Connection,
    mail_id: &str,
    from_project: Option<&str>,
    from_path: Option<&str>,
    to_project: &str,
    to_path: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO correction_log (mail_id, from_project, from_path, to_project, to_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![mail_id, from_project, from_path, to_project, to_path],
    )?;
    Ok(())
}

pub(crate) fn reassign_with_correction(
    conn: &Connection,
    mail_id: &str,
    from_project: &str,
    to_project: &str,
) -> Result<(), AppError> {
    // パスの解決は行の更新前に行う（from がこの後の処理で消える場合に備える）
    let from_path = crate::db::projects::project_path_string(conn, from_project)?;
    let to_path = crate::db::projects::project_path_string(conn, to_project)?;
    conn.execute(
        "UPDATE mail_project_assignments
         SET project_id = ?1, assigned_by = 'user', corrected_from = ?2
         WHERE mail_id = ?3",
        params![to_project, from_project, mail_id],
    )?;
    insert_correction(conn, mail_id, Some(from_project), Some(&from_path), to_project, &to_path)?;
    Ok(())
}

pub fn get_recent_corrections(
    conn: &Connection,
    account_id: &str,
    limit: u32,
) -> Result<Vec<crate::models::classifier::CorrectionEntry>, AppError> {
    // projects への JOIN はしない: 参照先が削除された訂正もスナップショットで返す
    let mut stmt = conn.prepare(
        "SELECT m.subject, cl.from_path, cl.to_path
         FROM correction_log cl
         JOIN mails m ON cl.mail_id = m.id
         WHERE m.account_id = ?1
         ORDER BY cl.corrected_at DESC, cl.id DESC
         LIMIT ?2",
    )?;
    let corrections = stmt
        .query_map(params![account_id, limit], |row| {
            Ok(crate::models::classifier::CorrectionEntry {
                mail_subject: row.get(0)?,
                from_path: row.get(1)?,
                to_path: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(corrections)
}
```

他の `insert_correction` 呼び出し（`approve_classification_in_tx` 等）も同様にパスを解決して渡す。`classifier/prompt.rs` の few-shot 行生成は `entry.from_project`/`entry.to_project` 参照を `entry.from_path.as_deref().unwrap_or("(unclassified)")` / `entry.to_path` に置き換える（行フォーマット自体は既存のまま）。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS（prompt のスナップショットテストが文言変化で落ちる場合はテスト側をパス表記に更新）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/classifier.rs src-tauri/src/db/assignments.rs src-tauri/src/classifier/prompt.rs
git commit -m "feat(classifier): 訂正ログをパススナップショット化し案件削除後もfew-shotを保存する"
```

---

## Task 5: マージの検証強化と子の付け替え

**Files:**
- Modify: `src-tauri/src/db/projects.rs`（merge_projects）
- Modify: `src-tauri/src/commands/project_commands.rs`（with_conn_mut → with_conn）

**Interfaces:**
- Consumes: `subtree_ids` / `project_path_string`（Task 2）、`reassign_with_correction`（Task 4）
- Produces: `merge_projects(conn: &Connection, source_id, target_id) -> Result<u32, AppError>`（**&mut 不要に変更**。`unchecked_transaction` を使う——PR B で UseCase の `ctx.with_conn` から呼ぶため）

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test]
fn test_merge_rejects_self_and_descendant_and_archived() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "mid", "埼玉", Some("root"));
    insert_child(&conn, "arch", "旧公演", None);
    archive_project(&conn, "arch").unwrap();

    assert!(merge_projects(&conn, "root", "root").is_err(), "self");
    assert!(merge_projects(&conn, "root", "mid").is_err(), "descendant target");
    assert!(merge_projects(&conn, "mid", "arch").is_err(), "archived target");
    assert!(merge_projects(&conn, "arch", "mid").is_err(), "archived source");
}

#[test]
fn test_merge_reparents_children_to_target() {
    let conn = setup_db();
    insert_child(&conn, "src", "旧ツアー", None);
    insert_child(&conn, "child", "埼玉", Some("src"));
    insert_child(&conn, "dst", "新ツアー", None);

    merge_projects(&conn, "src", "dst").unwrap();

    let parent: Option<String> = conn
        .query_row("SELECT parent_id FROM projects WHERE id = 'child'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(parent.as_deref(), Some("dst"));
    assert!(get_project(&conn, "src").is_err());
}

#[test]
fn test_merge_records_path_snapshots() {
    let conn = setup_db();
    insert_child(&conn, "src", "照明", None);
    insert_child(&conn, "dst", "音響", None);
    let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "S", "2026-07-18T10:00:00");
    crate::db::mails::insert_mail(&conn, &m).unwrap();
    assignments::assign_mail(&conn, "m1", "src", "ai", Some(0.9)).unwrap();

    merge_projects(&conn, "src", "dst").unwrap();

    let c = &assignments::get_recent_corrections(&conn, "acc1", 1).unwrap()[0];
    assert_eq!(c.from_path.as_deref(), Some("照明"));
    assert_eq!(c.to_path, "音響");
}
```

既存の merge テスト（`&mut conn` を渡している箇所）は `&conn` に更新する。

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::projects`
Expected: FAIL

- [ ] **Step 3: 実装**

```rust
/// source を target へ統合する。
/// 検証: self / 異アカウント / target が source の子孫 / どちらかがアーカイブ済み → 拒否。
/// 処理順（1トランザクション）: (1) source 直属メールを target へ reassign（パス
/// スナップショット付き訂正ログ）→ (2) source の子を target の子へ reparent →
/// (3) source を削除（子は移動済みなので単発 DELETE で CASCADE 再帰なし）。
pub fn merge_projects(
    conn: &Connection,
    source_id: &str,
    target_id: &str,
) -> Result<u32, AppError> {
    if source_id == target_id {
        return Err(AppError::Validation("同じ案件同士はマージできません".into()));
    }
    let source = get_project(conn, source_id)?;
    let target = get_project(conn, target_id)?;
    if source.account_id != target.account_id {
        return Err(AppError::Validation("異なるアカウントの案件はマージできません".into()));
    }
    if source.is_archived || target.is_archived {
        return Err(AppError::Validation("アーカイブ済みの案件はマージできません".into()));
    }
    if subtree_ids(conn, source_id)?.contains(&target_id.to_string()) {
        return Err(AppError::Validation("統合先が統合元の配下にあります".into()));
    }

    let tx = conn.unchecked_transaction()?;
    let mail_ids: Vec<String> = {
        let mut stmt =
            tx.prepare("SELECT mail_id FROM mail_project_assignments WHERE project_id = ?1")?;
        let ids = stmt
            .query_map(params![source_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };
    let count = mail_ids.len() as u32;
    for mail_id in &mail_ids {
        assignments::reassign_with_correction(&tx, mail_id, source_id, target_id)?;
    }
    tx.execute(
        "UPDATE projects SET parent_id = ?1 WHERE parent_id = ?2",
        params![target_id, source_id],
    )?;
    tx.execute("DELETE FROM projects WHERE id = ?1", params![source_id])?;
    tx.commit()?;
    Ok(count)
}
```

`commands/project_commands.rs` の `merge_projects` command は `state.with_conn(|conn| ...)` に変更（`with_conn_mut` 不要に）。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/projects.rs src-tauri/src/commands/project_commands.rs
git commit -m "fix(db): マージにself・子孫target・アーカイブ済み・異アカウントの検証と子案件の付け替えを追加"
```

---

## Task 6: 集約表示バックエンド（get_threads_by_project のサブツリー化）

**Files:**
- Modify: `src-tauri/src/models/mail.rs`（ThreadProjectRef / Thread.projects）
- Modify: `src-tauri/src/threading.rs`（Thread 構築時に `projects: Vec::new()`）
- Modify: `src-tauri/src/db/assignments.rs`（get_mails_by_projects）
- Modify: `src-tauri/src/db/mails.rs`（get_threads_by_project）

**Interfaces:**
- Consumes: `subtree_ids` / `project_path_string`（Task 2）
- Produces:
  - `pub struct ThreadProjectRef { pub project_id: String, pub display_path: String }`（Serialize/Deserialize）
  - `Thread.projects: Vec<ThreadProjectRef>` — メンバーメールの直接所属案件の集合。選択ノード直属のみなら空
  - `assignments::get_mails_by_projects(conn, project_ids: &[String]) -> Result<Vec<Mail>, AppError>`
  - `assignments::get_assignment_map(conn, project_ids: &[String]) -> Result<HashMap<String, String>, AppError>`（mail_id → project_id）
  - `mails::get_threads_by_project(conn, project_id)` — サブツリー集約 + projects 注釈。`display_path` は選択ノードからの**相対パス**

- [ ] **Step 1: 失敗するテストを書く**

`db/mails.rs` のテストに追加:

```rust
#[test]
fn test_get_threads_by_project_aggregates_subtree_with_relative_paths() {
    let conn = setup_db();
    crate::db::projects::insert_project_with_id(&conn, "root", "acc1", "ツアー", None, None, None).unwrap();
    crate::db::projects::insert_project_with_id(&conn, "mid", "acc1", "埼玉", None, None, Some("root")).unwrap();
    crate::db::projects::insert_project_with_id(&conn, "leaf", "acc1", "音響", None, None, Some("mid")).unwrap();

    for (mid, pid, subj) in [("m1", "root", "全体連絡"), ("m2", "leaf", "音響仕込み")] {
        let m = crate::test_helpers::make_mail(mid, &format!("<{mid}@ex>"), subj, "2026-07-18T10:00:00");
        crate::db::mails::insert_mail(&conn, &m).unwrap();
        crate::db::assignments::assign_mail(&conn, mid, pid, "user", None).unwrap();
    }

    // root 選択: 両方のメールが見え、leaf 所属スレッドには相対パスチップ
    let threads = get_threads_by_project(&conn, "root").unwrap();
    assert_eq!(threads.len(), 2);
    let leaf_thread = threads.iter().find(|t| t.subject == "音響仕込み").unwrap();
    assert_eq!(leaf_thread.projects.len(), 1);
    assert_eq!(leaf_thread.projects[0].display_path, "埼玉 > 音響");
    let root_thread = threads.iter().find(|t| t.subject == "全体連絡").unwrap();
    assert!(root_thread.projects.is_empty(), "選択ノード直属はチップなし");

    // leaf 選択: 自分の1通のみ
    let threads = get_threads_by_project(&conn, "leaf").unwrap();
    assert_eq!(threads.len(), 1);
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::mails`
Expected: FAIL（コンパイルエラー: Thread.projects / ThreadProjectRef 未定義）

- [ ] **Step 3: 実装**

`models/mail.rs`:

```rust
/// 集約表示でスレッドに付ける「どの案件のメールか」の注釈。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadProjectRef {
    pub project_id: String,
    /// 選択ノードからの相対パス（例: 選択が「ツアー」なら "埼玉 > 音響"）。
    /// 階層内では同名案件が共存し得るため単一 name ではなくパスにする。
    pub display_path: String,
}
```

`Thread` に `pub projects: Vec<ThreadProjectRef>,` を追加。`threading.rs::build_threads` の `Thread { ... }` 構築に `projects: Vec::new(),` を追加。

`db/assignments.rs`（IN 句は可変長のためプレースホルダを組み立てる。既存 `get_mails_by_project` の SELECT 句・ORDER と同じ形を IN に一般化する）:

```rust
/// 複数案件（サブツリー展開済み ID 集合）に所属するメールを一括取得する。
pub fn get_mails_by_projects(conn: &Connection, project_ids: &[String]) -> Result<Vec<Mail>, AppError> {
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; project_ids.len()].join(",");
    let sql = format!(
        "SELECT {MAIL_COLUMNS} FROM mails m
         JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
         WHERE mpa.project_id IN ({placeholders})
         ORDER BY m.date DESC"
    );
    // MAIL_COLUMNS: 既存 get_mails_by_project の SELECT 句をそのまま使う
    // （定数が無ければこのタスクで文字列定数に括り出して両関数で共有する）
    let mut stmt = conn.prepare(&sql)?;
    let mails = stmt
        .query_map(rusqlite::params_from_iter(project_ids.iter()), row_to_mail)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mails)
}

/// mail_id → 直接所属 project_id の対応表（集約表示の注釈用）。
pub fn get_assignment_map(
    conn: &Connection,
    project_ids: &[String],
) -> Result<std::collections::HashMap<String, String>, AppError> {
    if project_ids.is_empty() {
        return Ok(Default::default());
    }
    let placeholders = vec!["?"; project_ids.len()].join(",");
    let sql = format!(
        "SELECT mail_id, project_id FROM mail_project_assignments WHERE project_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let map = stmt
        .query_map(rusqlite::params_from_iter(project_ids.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;
    Ok(map)
}
```

（`row_to_mail` 相当のマッパーが関数ローカルの場合は共有ヘルパへ括り出す。既存 `get_mails_by_project` は `get_mails_by_projects(&[project_id.to_string()])` への委譲に書き換えて重複を消す）

`db/mails.rs`:

```rust
pub fn get_threads_by_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<Thread>, AppError> {
    let ids = crate::db::projects::subtree_ids(conn, project_id)?;
    if ids.is_empty() {
        return Err(AppError::ProjectNotFound(project_id.to_string()));
    }
    let mails = assignments::get_mails_by_projects(conn, &ids)?;
    let assignment_map = assignments::get_assignment_map(conn, &ids)?;

    // 選択ノードからの相対パス表（選択ノード自身は注釈対象外）
    let selected_path = crate::db::projects::project_path_string(conn, project_id)?;
    let prefix = format!("{selected_path} > ");
    let mut rel_paths: std::collections::HashMap<String, String> = Default::default();
    for pid in ids.iter().filter(|pid| pid.as_str() != project_id) {
        let full = crate::db::projects::project_path_string(conn, pid)?;
        let rel = full.strip_prefix(&prefix).unwrap_or(&full).to_string();
        rel_paths.insert(pid.clone(), rel);
    }

    let mut threads = crate::threading::build_threads(mails);
    for thread in &mut threads {
        let mut seen = std::collections::HashSet::new();
        for mail in &thread.mails {
            if let Some(pid) = assignment_map.get(&mail.id) {
                if pid != project_id && seen.insert(pid.clone()) {
                    if let Some(rel) = rel_paths.get(pid) {
                        thread.projects.push(ThreadProjectRef {
                            project_id: pid.clone(),
                            display_path: rel.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(threads)
}
```

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS（Thread を構築する既存テストは `projects: Vec::new()` 追加で追従）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/mail.rs src-tauri/src/threading.rs src-tauri/src/db/assignments.rs src-tauri/src/db/mails.rs
git commit -m "feat(db): 案件スレッド一覧をサブツリー集約にし直接所属案件の相対パス注釈を追加"
```

---

## Task 7: 加算的継承（build_effective_context）

**Files:**
- Modify: `src-tauri/src/db/projects.rs`
- Modify: `src-tauri/src/commands/project_commands.rs`（get_effective_context command）
- Modify: `src-tauri/src/lib.rs`（command 登録）

**Interfaces:**
- Consumes: `ancestor_path`（Task 2）、`crate::db::project_contexts::get_context`、`crate::db::project_directories` の取得関数（実ファイルの関数名に合わせる）
- Produces:
  - `pub struct EffectiveContextEntry { pub project_id: String, pub project_name: String, pub is_self: bool, pub directory_path: Option<String>, pub context: Option<String> }`（Serialize）
  - `pub fn build_effective_context(conn, project_id) -> Result<Vec<EffectiveContextEntry>, AppError>` — ルート→自ノード順。各項目に定義元ノードを付けて返す（クラウド送信可否は定義元ノードのルールに従うため、ルールの合成はしない——設計書 §7）
  - Tauri command `get_effective_context(project_id) -> Vec<EffectiveContextEntry>`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test]
fn test_build_effective_context_accumulates_ancestors() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "leaf", "音響", Some("root"));
    crate::db::project_contexts::upsert_generated(&conn, "root", "ツアー全体の共有情報", "h", "i").unwrap();
    crate::db::project_contexts::upsert_generated(&conn, "leaf", "音響の機材リスト", "h", "i").unwrap();

    let entries = build_effective_context(&conn, "leaf").unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].project_name, "ツアー");
    assert!(!entries[0].is_self);
    assert_eq!(entries[0].context.as_deref(), Some("ツアー全体の共有情報"));
    assert_eq!(entries[1].project_name, "音響");
    assert!(entries[1].is_self);
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::projects`
Expected: FAIL

- [ ] **Step 3: 実装**

```rust
/// 加算的継承の有効コンテキスト。ルート→自ノード順で、各エントリは定義元
/// ノードに紐づく（クラウド送信可否も定義元ノードのルールで評価される——
/// ルール同士の合成はしない。設計書 §7）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectiveContextEntry {
    pub project_id: String,
    pub project_name: String,
    pub is_self: bool,
    pub directory_path: Option<String>,
    pub context: Option<String>,
}

pub fn build_effective_context(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<EffectiveContextEntry>, AppError> {
    let path = ancestor_path(conn, project_id)?;
    if path.is_empty() {
        return Err(AppError::ProjectNotFound(project_id.to_string()));
    }
    let mut entries = Vec::with_capacity(path.len());
    for node in &path {
        let context = crate::db::project_contexts::get_context(conn, &node.id)?
            .and_then(|c| c.cached_context);
        // ディレクトリ取得は db::project_directories の既存取得関数を使う
        // （関数名は実ファイルに合わせる。パスのみ取り出す）
        let directory_path = crate::db::project_directories::get_directory(conn, &node.id)?
            .map(|d| d.path);
        entries.push(EffectiveContextEntry {
            project_id: node.id.clone(),
            project_name: node.name.clone(),
            is_self: node.id == project_id,
            directory_path,
            context,
        });
    }
    Ok(entries)
}
```

command（既存 project_commands の非 dispatch 流儀で追加し、PR B のタスクで read 系はそのまま残す——Read 系 UseCase 化は本設計のスコープ外）:

```rust
#[tauri::command]
pub fn get_effective_context(
    state: State<DbState>,
    project_id: String,
) -> Result<Vec<projects::EffectiveContextEntry>, AppError> {
    state.with_conn(|conn| projects::build_effective_context(conn, &project_id))
}
```

`lib.rs` の `invoke_handler` 一覧に `get_effective_context` を追加。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/projects.rs src-tauri/src/commands/project_commands.rs src-tauri/src/lib.rs
git commit -m "feat(db): 祖先パスに沿った加算的な有効コンテキスト取得を追加"
```

**→ ここまでで PR A を作成**（ブランチ `feat/project-hierarchy-db`、base: main）。PRタイトル例: 「案件を階層構造にするDB基盤（parent_id・サブツリー操作・訂正ログのスナップショット化）」

---

## Task 8: 操作系 UseCase 化と dispatch 移行（PR B）

**Files:**
- Create: `src-tauri/src/usecase/cases/project.rs`
- Modify: `src-tauri/src/usecase/cases/mod.rs`（register_all に追加）
- Modify: `src-tauri/src/commands/project_commands.rs`（dispatch 経由へ書き換え + set_project_parent + delete 確認用件数）
- Modify: `src-tauri/src/lib.rs`（set_project_parent / get_project_delete_impact 登録）

**Interfaces:**
- Consumes: Task 2〜5 の db 関数、`crate::usecase::{Registry, Risk, UseCase}`、`crate::context::Ctx`、dispatch 呼び出しパターン（`commands/search_commands.rs` の `search_mails` と同型: `Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks)` → `dispatch(&registry, name, json!, &ctx)` → `serde_json::from_value`）
- Produces:
  - UseCase 名: `create_project` / `update_project` / `set_project_parent` / `archive_project` / `delete_project` / `merge_projects`
  - Risk: create/update/set_parent = `Reversible`、archive/delete/merge = `Sensitive`（設計書 §5 の表）
  - Tauri command `set_project_parent(project_id, parent_id: Option<String>)`
  - Tauri command `get_project_delete_impact(project_id) -> DeleteImpact { projects: u32, mails: u32 }`（Read 系・確認ダイアログ用）

- [ ] **Step 1: 失敗するテストを書く**

`usecase/cases/project.rs` を新規作成（スタブ＋テスト。テストの流儀は `cases/assign.rs` の `build_states` / `Ctx::new_for_test` と同じ）:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::db::projects;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::{Driver, Risk, UseCase};

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[tokio::test]
    async fn test_create_project_usecase_with_parent() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            projects::insert_project_with_id(conn, "root", "acc1", "ツアー", None, None, None)?;
            Ok(())
        })
        .unwrap();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);

        let out = CreateProjectUseCase
            .run(
                CreateProjectInput {
                    account_id: "acc1".into(),
                    name: "埼玉".into(),
                    description: None,
                    color: None,
                    parent_id: Some("root".into()),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(out.parent_id.as_deref(), Some("root"));
    }

    #[tokio::test]
    async fn test_set_project_parent_usecase() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            projects::insert_project_with_id(conn, "a", "acc1", "A", None, None, None)?;
            projects::insert_project_with_id(conn, "b", "acc1", "B", None, None, None)?;
            Ok(())
        })
        .unwrap();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);

        SetProjectParentUseCase
            .run(
                SetProjectParentInput { project_id: "b".into(), parent_id: Some("a".into()) },
                &ctx,
            )
            .await
            .unwrap();
        let parent = db
            .with_conn(|conn| Ok(projects::get_project(conn, "b")?.parent_id))
            .unwrap();
        assert_eq!(parent.as_deref(), Some("a"));
    }

    #[test]
    fn test_risk_matrix_matches_design() {
        // 設計書 §5: create/update/set_parent = Reversible、archive/delete/merge = Sensitive
        let (db, pending, batches, locks) = build_states();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);
        let dummy_create = CreateProjectInput {
            account_id: "acc1".into(), name: "n".into(),
            description: None, color: None, parent_id: None,
        };
        assert_eq!(CreateProjectUseCase.risk(&dummy_create, &ctx).unwrap(), Risk::Reversible);
        assert_eq!(
            DeleteProjectUseCase.risk(&DeleteProjectInput { project_id: "x".into() }, &ctx).unwrap(),
            Risk::Sensitive
        );
        assert_eq!(
            MergeProjectsUseCase
                .risk(&MergeProjectsInput { source_id: "a".into(), target_id: "b".into() }, &ctx)
                .unwrap(),
            Risk::Sensitive
        );
        assert_eq!(
            ArchiveProjectUseCase.risk(&ArchiveProjectInput { project_id: "x".into() }, &ctx).unwrap(),
            Risk::Sensitive
        );
    }
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test usecase::cases::project`
Expected: FAIL（コンパイルエラー）

- [ ] **Step 3: 実装**

`usecase/cases/project.rs`（6 UseCase。全て db 関数への薄い委譲。Input は `#[derive(Deserialize)]`、Output は Serialize 可能な型）:

```rust
//! 案件構造の変更系 use case。階層を変更できる操作は全てここに集約し
//! dispatch バス（ADR 0004）経由にする——経路を割ると認可・監査が分裂するため。
//! Risk は設計書 §5 の表に従う: 作成/更新/付け替えは Reversible、
//! アーカイブ（復元なし）/削除（サブツリー+付随データ）/マージ（source 削除）は Sensitive。

use serde::Deserialize;

use crate::context::Ctx;
use crate::db::projects;
use crate::error::AppError;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize)]
pub struct CreateProjectInput {
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub parent_id: Option<String>,
}

pub struct CreateProjectUseCase;

#[async_trait::async_trait]
impl UseCase for CreateProjectUseCase {
    type Input = CreateProjectInput;
    type Output = Project;
    fn name(&self) -> &'static str {
        "create_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let req = CreateProjectRequest {
            account_id: input.account_id,
            name: input.name,
            description: input.description,
            color: input.color,
            parent_id: input.parent_id,
        };
        ctx.with_conn(|conn| projects::insert_project(conn, &req))
    }
}

#[derive(Deserialize)]
pub struct UpdateProjectInput {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
}

pub struct UpdateProjectUseCase;

#[async_trait::async_trait]
impl UseCase for UpdateProjectUseCase {
    type Input = UpdateProjectInput;
    type Output = Project;
    fn name(&self) -> &'static str {
        "update_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let req = UpdateProjectRequest {
            name: input.name,
            description: input.description,
            color: input.color,
        };
        ctx.with_conn(|conn| projects::update_project(conn, &input.id, &req))
    }
}

#[derive(Deserialize)]
pub struct SetProjectParentInput {
    pub project_id: String,
    pub parent_id: Option<String>,
}

pub struct SetProjectParentUseCase;

#[async_trait::async_trait]
impl UseCase for SetProjectParentUseCase {
    type Input = SetProjectParentInput;
    type Output = ();
    fn name(&self) -> &'static str {
        "set_project_parent"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::set_parent(conn, &input.project_id, input.parent_id.as_deref()))
    }
}

#[derive(Deserialize)]
pub struct ArchiveProjectInput {
    pub project_id: String,
}

pub struct ArchiveProjectUseCase;

#[async_trait::async_trait]
impl UseCase for ArchiveProjectUseCase {
    type Input = ArchiveProjectInput;
    type Output = ();
    fn name(&self) -> &'static str {
        "archive_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        // 復元 use case が無いため実質不可逆（設計書 §5）
        Ok(Risk::Sensitive)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::archive_project(conn, &input.project_id))
    }
}

#[derive(Deserialize)]
pub struct DeleteProjectInput {
    pub project_id: String,
}

pub struct DeleteProjectUseCase;

#[async_trait::async_trait]
impl UseCase for DeleteProjectUseCase {
    type Input = DeleteProjectInput;
    type Output = ();
    fn name(&self) -> &'static str {
        "delete_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Sensitive)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::delete_project(conn, &input.project_id))
    }
}

#[derive(Deserialize)]
pub struct MergeProjectsInput {
    pub source_id: String,
    pub target_id: String,
}

pub struct MergeProjectsUseCase;

#[async_trait::async_trait]
impl UseCase for MergeProjectsUseCase {
    type Input = MergeProjectsInput;
    type Output = u32;
    fn name(&self) -> &'static str {
        "merge_projects"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Sensitive)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::merge_projects(conn, &input.source_id, &input.target_id))
    }
}

pub fn register_project_cases(registry: &mut Registry) {
    registry.register(CreateProjectUseCase);
    registry.register(UpdateProjectUseCase);
    registry.register(SetProjectParentUseCase);
    registry.register(ArchiveProjectUseCase);
    registry.register(DeleteProjectUseCase);
    registry.register(MergeProjectsUseCase);
}
```

`cases/mod.rs` に `pub mod project;` と `project::register_project_cases(registry);` を追加。

`commands/project_commands.rs` を dispatch 経由に書き換え（`search_mails` command と同型。create の例——update/archive/delete/merge/set_parent も同じ形）:

```rust
#[tauri::command]
pub async fn create_project(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    name: String,
    description: Option<String>,
    color: Option<String>,
    parent_id: Option<String>,
) -> Result<Project, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "create_project",
        serde_json::json!({
            "account_id": account_id, "name": name, "description": description,
            "color": color, "parent_id": parent_id,
        }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected create_project output: {e}")))
}
```

Read 系はそのまま（`get_projects` / `get_effective_context`）。確認ダイアログ用の Read command を追加:

```rust
#[derive(serde::Serialize)]
pub struct DeleteImpact {
    pub projects: u32,
    pub mails: u32,
}

#[tauri::command]
pub fn get_project_delete_impact(
    state: State<DbState>,
    project_id: String,
) -> Result<DeleteImpact, AppError> {
    state.with_conn(|conn| {
        let ids = projects::subtree_ids(conn, &project_id)?;
        if ids.is_empty() {
            return Err(AppError::ProjectNotFound(project_id.clone()));
        }
        Ok(DeleteImpact {
            projects: ids.len() as u32,
            mails: projects::count_subtree_mails(conn, &project_id)?,
        })
    })
}
```

`lib.rs`: `set_project_parent` / `get_project_delete_impact` を invoke_handler へ追加。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/usecase/cases/project.rs src-tauri/src/usecase/cases/mod.rs src-tauri/src/commands/project_commands.rs src-tauri/src/lib.rs
git commit -m "feat(ui): 案件の構造変更操作をUseCase化しdispatchバス経由に統一（親の付け替えを追加）"
```

**→ PR B を作成**（ブランチ `feat/project-hierarchy-usecases`、base: PR A）

---

## Task 9: 分類プロンプトのパス対応（PR C）

**Files:**
- Modify: `src-tauri/src/models/classifier.rs`（ProjectSummary.path）
- Modify: `src-tauri/src/db/projects.rs`（build_project_summaries）
- Modify: `src-tauri/src/classifier/prompt.rs`

**Interfaces:**
- Produces:
  - `ProjectSummary.path: String`（フルパス。「ツアー > 埼玉 > 音響」）
  - `build_project_summaries` — 全ノード分の path を**1クエリ+メモリ合成**で付与（ノード数ぶんの再帰CTEを打たない）
  - プロンプト: 案件行が `- id: {id}, path: {path}, description: ...`、SYSTEM_PROMPT に最深確信ノードの指示

- [ ] **Step 1: 失敗するテストを書く**

`db/projects.rs`:

```rust
#[test]
fn test_build_project_summaries_includes_paths() {
    let conn = setup_db();
    insert_child(&conn, "root", "ツアー", None);
    insert_child(&conn, "leaf", "音響", Some("root"));

    let summaries = build_project_summaries(&conn, "acc1", false).unwrap();
    let leaf = summaries.iter().find(|s| s.id == "leaf").unwrap();
    assert_eq!(leaf.path, "ツアー > 音響");
    let root = summaries.iter().find(|s| s.id == "root").unwrap();
    assert_eq!(root.path, "ツアー");
}
```

`classifier/prompt.rs`:

```rust
#[test]
fn test_user_prompt_lists_projects_with_path() {
    let projects = vec![ProjectSummary {
        id: "leaf".into(),
        name: "音響".into(),
        path: "ツアー > 音響".into(),
        description: None,
        recent_subjects: vec![],
        top_senders: vec![],
        context: None,
    }];
    let prompt = build_user_prompt(&sample_mail(), &projects, &[]);
    assert!(prompt.contains("path: ツアー > 音響"), "{prompt}");
}

#[test]
fn test_system_prompt_instructs_deepest_confident_node() {
    assert!(SYSTEM_PROMPT.contains("deepest"));
}
```

（`sample_mail()` は既存プロンプトテストのヘルパを使う。無ければ既存テストが作っている MailSummary 生成をそのまま流用）

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test classifier && cargo test db::projects`
Expected: FAIL

- [ ] **Step 3: 実装**

`ProjectSummary` に `pub path: String,` を追加。`build_project_summaries` の path 合成:

```rust
pub fn build_project_summaries(
    conn: &Connection,
    account_id: &str,
    for_cloud: bool,
) -> Result<Vec<ProjectSummary>, AppError> {
    let projs = list_projects(conn, account_id)?;
    // 全ノードのパスを1回のメモリ合成で作る（ノード数ぶんの再帰CTEを打たない）。
    // list_projects はアーカイブ除外だが、パス合成の親参照は全件必要なため別途取得
    let mut names: std::collections::HashMap<String, (String, Option<String>)> = Default::default();
    {
        let mut stmt =
            conn.prepare("SELECT id, name, parent_id FROM projects WHERE account_id = ?1")?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))
        })?;
        for row in rows {
            let (id, v) = row?;
            names.insert(id, v);
        }
    }
    let path_of = |id: &str| -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(c) = cur {
            match names.get(&c) {
                Some((name, parent)) => {
                    parts.push(name.clone());
                    cur = parent.clone();
                }
                None => break,
            }
        }
        parts.reverse();
        parts.join(" > ")
    };
    let mut summaries = Vec::with_capacity(projs.len());
    for p in projs {
        let recent_subjects = assignments::get_recent_subjects(conn, &p.id, 10)?;
        let top_senders = assignments::get_top_senders(conn, &p.id, 5)?;
        let context = crate::db::project_contexts::get_context(conn, &p.id)?
            .filter(|c| !for_cloud || c.allow_cloud_context)
            .and_then(|c| c.cached_context)
            .map(|c| c.chars().take(800).collect::<String>());
        summaries.push(ProjectSummary {
            path: path_of(&p.id),
            id: p.id,
            name: p.name,
            description: p.description,
            recent_subjects,
            top_senders,
            context,
        });
    }
    Ok(summaries)
}
```

`prompt.rs` の案件行を `- id: {id}, path: {path}, description: ...` に変更（name 単独表記をやめる）。`SYSTEM_PROMPT` に追記（既存の assign 説明の近く）:

```text
Projects form a hierarchy shown as "path" (e.g. "Tour > Venue > Sound").
Assign to the deepest node you are confident about.
If you cannot decide between child nodes, assign to their parent instead.
```

`ProjectSummary` を構築している他のテスト・コードには `path` フィールドを追加（フラット案件は `path == name`）。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/classifier.rs src-tauri/src/db/projects.rs src-tauri/src/classifier/prompt.rs
git commit -m "feat(classifier): 案件リストをパス付きで提示し最深確信ノードへの割当を指示"
```

---

## Task 10: create with parent（子案件の作成提案）

**Files:**
- Modify: `src-tauri/src/models/classifier.rs`（ClassifyAction::Create）
- Modify: `src-tauri/src/classifier/prompt.rs`（SYSTEM_PROMPT の create 形式）
- Modify: `src-tauri/src/classifier/service.rs`（apply_result の parent 検証・pending への保持）
- Modify: `src-tauri/src/commands/classify_commands.rs`（approve_new_project）

**Interfaces:**
- Produces:
  - `ClassifyAction::Create { project_name: String, description: String, #[serde(default)] parent_project_id: Option<String> }`
  - `apply_result`: parent_project_id が存在しない/別アカウントなら **None に落としてルート作成として扱う**（create はユーザー承認制のため安全——設計書 §6）
  - `approve_new_project` command が `parent_project_id: Option<String>` を受け取り、作成時に親として使う
  - pending（PendingClassifications）に積む提案へ parent_project_id を保持（既存の提案構造体にフィールド追加。実ファイルの構造体名に合わせる）

- [ ] **Step 1: 失敗するテストを書く**

`classifier/service.rs` のテスト（既存の apply_result / StubLlm テストの流儀に合わせる）:

```rust
#[test]
fn test_create_action_parses_parent_project_id() {
    let json = r#"{"action":"create","project_name":"音響","description":"d","parent_project_id":"root","confidence":0.8,"reason":"r"}"#;
    let result: ClassifyResult = serde_json::from_str(json).unwrap();
    match result.action {
        ClassifyAction::Create { parent_project_id, .. } => {
            assert_eq!(parent_project_id.as_deref(), Some("root"));
        }
        _ => panic!("expected create"),
    }
}

#[test]
fn test_create_action_without_parent_still_parses() {
    // 旧形式の応答（parent なし）も壊れない
    let json = r#"{"action":"create","project_name":"音響","description":"d","confidence":0.8,"reason":"r"}"#;
    let result: ClassifyResult = serde_json::from_str(json).unwrap();
    match result.action {
        ClassifyAction::Create { parent_project_id, .. } => assert!(parent_project_id.is_none()),
        _ => panic!("expected create"),
    }
}

#[test]
fn test_validate_parent_project_falls_back_to_root_on_hallucination() {
    // 存在しない/別アカウントの parent_project_id は None に落とす
    // （検証ロジックを純関数 validate_parent_project として service.rs に切り出してテストする）
    let conn = crate::test_helpers::setup_db();
    crate::db::projects::insert_project_with_id(&conn, "root", "acc1", "ツアー", None, None, None)
        .unwrap();
    assert_eq!(
        validate_parent_project(&conn, "acc1", Some("root")),
        Some("root".to_string()),
        "実在する同一アカウントの親は通す"
    );
    assert_eq!(
        validate_parent_project(&conn, "acc1", Some("ghost")),
        None,
        "存在しない親はルート作成に落とす"
    );
    assert_eq!(validate_parent_project(&conn, "acc1", None), None);
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test classifier`
Expected: FAIL（フィールド未定義でコンパイルエラー）

- [ ] **Step 3: 実装**

- `ClassifyAction::Create` に `#[serde(default)] pub parent_project_id: Option<String>` を追加（構造体バリアントのフィールドに `#[serde(default)]` を付ける）
- `SYSTEM_PROMPT` の create JSON 例に `"parent_project_id": "<existing project id or omit for a root project>"` を追加し、「サブトピックのメールは既存案件の下に create してよい」と1文追記
- `service.rs` に検証関数を切り出す:

```rust
/// create 提案の parent_project_id を検証する。存在しない・別アカウントの場合は
/// None（=ルート作成）に落とす。create はユーザー承認制のためエラーにはしない。
pub(crate) fn validate_parent_project(
    conn: &rusqlite::Connection,
    account_id: &str,
    parent_project_id: Option<&str>,
) -> Option<String> {
    let pid = parent_project_id?;
    match crate::db::projects::get_project(conn, pid) {
        Ok(p) if p.account_id == account_id => Some(p.id),
        _ => None,
    }
}
```

- `apply_result` の Create 分岐でこれを通し、結果を pending に積む提案構造体へ保持（失敗時は None に落として続行、エラーにしない）
- `approve_new_project` command: 引数に `parent_project_id: Option<String>` を追加し、`insert_project_with_id(conn, &id, &account_id, &name, desc, color, parent_project_id.as_deref())` へ渡す（アーカイブ済み親は insert 側の検証で拒否される）
- フロントの承認ダイアログ（`NewProjectProposal.tsx` 等）への作成位置表示は PR E（Task 14）で行う。この時点では API 形だけ拡張

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test && pnpm tsc --noEmit`
Expected: PASS（TS は API 未使用のため影響なし）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/classifier.rs src-tauri/src/classifier/prompt.rs src-tauri/src/classifier/service.rs src-tauri/src/commands/classify_commands.rs
git commit -m "feat(classifier): 既存案件配下への子案件作成提案（create with parent）を追加"
```

**→ PR C を作成**（ブランチ `feat/project-hierarchy-classifier`、base: PR B）

---

## Task 11: 検索のサブツリースコープ（PR D）

**Files:**
- Modify: `src-tauri/src/db/search.rs`（search_mails / search_fts / search_like）
- Modify: `src-tauri/src/db/vec_search.rs`（search_mails_semantic）
- Modify: `src-tauri/src/usecase/cases/search.rs`（Input に project_id）
- Modify: `src-tauri/src/commands/search_commands.rs`（引数配線）
- Modify: `src/api/searchApi.ts`（スコープ引数）

**Interfaces:**
- Consumes: dispatch 経由の検索経路（既存）
- Produces:
  - `search_mails(conn, account_id, query, project_id: Option<&str>, limit)` — スコープ指定時はサブツリー内のみ（未分類は対象外）
  - `search_mails_semantic(conn, account_id, query_embedding, project_id: Option<&str>, limit)` — 同上。**スコープ指定時は KNN の k を KNN_MAX まで拡大**（KNN後フィルタの取りこぼし緩和。残る制限は既知の制限としてコメントに明記——設計書 §8）
  - UseCase 入力に `project_id: Option<String>`（serde default）
  - `searchApi.searchMails(accountId, query, projectId?)` / `searchApi.semanticSearch(accountId, query, projectId?)`

- [ ] **Step 1: 失敗するテストを書く**

`db/search.rs` のテストに追加（既存の FTS テストのセットアップ流儀に合わせる）:

```rust
#[test]
fn test_search_scoped_to_subtree() {
    let conn = setup_db();
    crate::db::projects::insert_project_with_id(&conn, "root", "acc1", "ツアー", None, None, None).unwrap();
    crate::db::projects::insert_project_with_id(&conn, "leaf", "acc1", "音響", None, None, Some("root")).unwrap();
    crate::db::projects::insert_project_with_id(&conn, "other", "acc1", "別件", None, None, None).unwrap();

    for (mid, pid, subj) in [
        ("m1", Some("leaf"), "スピーカー設営の件"),
        ("m2", Some("other"), "スピーカー購入の件"),
        ("m3", None, "スピーカー無関係未分類"),
    ] {
        let mut m = crate::test_helpers::make_mail(mid, &format!("<{mid}@ex>"), subj, "2026-07-18T10:00:00");
        m.body_text = Some("スピーカー".into());
        crate::db::mails::insert_mail(&conn, &m).unwrap();
        crate::db::fts::index_mail(&conn, &m).unwrap(); // 既存FTSテストの索引投入の流儀に合わせる
        if let Some(pid) = pid {
            crate::db::assignments::assign_mail(&conn, mid, pid, "user", None).unwrap();
        }
    }

    // スコープなし: 3件
    let all = search_mails(&conn, "acc1", "スピーカー", None, 50).unwrap();
    assert_eq!(all.len(), 3);
    // root スコープ: サブツリー内の m1 のみ（未分類 m3 は含まれない）
    let scoped = search_mails(&conn, "acc1", "スピーカー", Some("root"), 50).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].mail.id, "m1");
}
```

`db/vec_search.rs` にも同型のテスト（FakeEmbedder / 既存テストのベクトル投入の流儀で、スコープ指定時に対象外案件のメールが返らないこと + `project_id: None` で従来挙動）。

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::search db::vec_search`
Expected: FAIL（引数不一致のコンパイルエラー）

- [ ] **Step 3: 実装**

`search.rs`: `search_mails` / `search_fts` / `search_like` に `project_id: Option<&str>` を追加。WHERE 句に共通の述語を足す（LEFT JOIN は既存のまま。スコープ時は実質 INNER になる）:

```sql
AND (?N IS NULL OR mpa.project_id IN (
    WITH RECURSIVE scope(id) AS (
        SELECT id FROM projects WHERE id = ?N
        UNION ALL
        SELECT p.id FROM projects p JOIN scope s ON p.parent_id = s.id
    )
    SELECT id FROM scope
))
```

（`?N` は同一パラメータを2箇所で使うため番号付きプレースホルダで同番号を指定する）

`vec_search.rs`: 同じ述語を mail 情報の取得クエリに追加し、k の決定を変更:

```rust
// スコープ指定時は k を常に上限まで拡大する。KNN→後段フィルタ方式のため、
// 上位 k 件をスコープ外が占有すると取りこぼす（既知の制限、設計書 §8）。
// 小さい案件では k=KNN_MAX でも取りこぼしが残り得ることを許容する。
let k = if project_id.is_some() {
    KNN_MAX
} else {
    /* 既存の k 計算式をそのまま */
};
```

`usecase/cases/search.rs` の Input 構造体2つ（全文/セマンティック）に `#[serde(default)] pub project_id: Option<String>` を追加し db 関数へ配線。`commands/search_commands.rs` の両 command に `project_id: Option<String>` 引数を追加して json! へ含める。`src/api/searchApi.ts` のラッパに省略可能引数 `projectId?: string` を追加して `invoke` の引数へ `projectId` を渡す。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test && pnpm tsc --noEmit`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/search.rs src-tauri/src/db/vec_search.rs src-tauri/src/usecase/cases/search.rs src-tauri/src/commands/search_commands.rs src/api/searchApi.ts
git commit -m "feat(search): 選択案件のサブツリーに限定する検索スコープを追加（FTS・セマンティック両対応）"
```

**→ PR D を作成**（ブランチ `feat/project-hierarchy-search`、base: PR B）

---

## Task 12: フロント基盤（型・API・ストアのツリー化と状態整合）

**Files:**
- Modify: `src/types/project.ts` / `src/types/mail.ts`
- Modify: `src/api/projectApi.ts`
- Modify: `src/stores/projectStore.ts`
- Test: `src/__tests__/projectStoreTree.test.ts`（新規）

**Interfaces:**
- Produces:
  - `Project.parent_id: string | null` / `Thread.projects: ThreadProjectRef[]` / `ThreadProjectRef { project_id, display_path }`
  - `projectApi.setProjectParent(projectId, parentId: string | null)` / `projectApi.createProject(..., parentId?)` / `projectApi.getProjectDeleteImpact(projectId)` / `projectApi.getEffectiveContext(projectId)`
  - `projectStore`:
    - `buildTree(): ProjectTreeNode[]`（`ProjectTreeNode = { project: Project; children: ProjectTreeNode[] }`。ルート=parent_id null。created_at 順は現行踏襲）
    - `expandedIds: Set<string>` + `toggleExpanded(id)`（localStorage 永続化）
    - `aggregateUnread(counts: Record<string, number>): Record<string, number>`（ボトムアップ加算——表示値 = 自分+子孫）
    - `setProjectParent(projectId, parentId)` — 成功後 `fetchProjects` 再取得
    - 構造変更操作（create/setParent/archive/delete/merge）は成功後に一覧再取得+消えた id のキャッシュ（directories/contexts/scanningProjects）掃除+選択解除
    - `fetchDirectory`/`fetchProjectContext` はレスポンス反映前に対象 id が `projects` に存在するか確認し、無ければ破棄（遅延レスポンス競合の防止）

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/projectStoreTree.test.ts`:

```typescript
import { describe, expect, it } from "vitest";
import { buildProjectTree, aggregateUnread } from "../stores/projectTree";

const p = (id: string, parent: string | null): Project => ({
  id, account_id: "acc1", name: id, description: null, color: null,
  is_archived: false, parent_id: parent,
  created_at: "2026-07-18", updated_at: "2026-07-18",
});

describe("buildProjectTree", () => {
  it("builds nested tree from flat array", () => {
    const tree = buildProjectTree([p("root", null), p("mid", "root"), p("leaf", "mid"), p("other", null)]);
    expect(tree).toHaveLength(2);
    const root = tree.find((n) => n.project.id === "root")!;
    expect(root.children[0].project.id).toBe("mid");
    expect(root.children[0].children[0].project.id).toBe("leaf");
  });

  it("treats orphan parent_id as root (archived ancestor not in list)", () => {
    const tree = buildProjectTree([p("child", "gone")]);
    expect(tree).toHaveLength(1);
  });
});

describe("aggregateUnread", () => {
  it("sums descendants bottom-up", () => {
    const projects = [p("root", null), p("mid", "root"), p("leaf", "mid")];
    const agg = aggregateUnread(projects, { root: 1, mid: 2, leaf: 3 });
    expect(agg).toEqual({ root: 6, mid: 5, leaf: 3 });
  });
});
```

（ツリー構築と集約は純関数として `src/stores/projectTree.ts` に切り出す——ストア本体から分離してテスト可能にする）

- [ ] **Step 2: Red を確認**

Run: `pnpm test projectStoreTree`
Expected: FAIL（モジュール未定義）

- [ ] **Step 3: 実装**

`src/stores/projectTree.ts`（新規・純関数）:

```typescript
import type { Project } from "../types/project";

export interface ProjectTreeNode {
  project: Project;
  children: ProjectTreeNode[];
}

/** フラット配列から木を組む。親が配列内に居ない場合（アーカイブ済み祖先等）はルート扱い。 */
export function buildProjectTree(projects: Project[]): ProjectTreeNode[] {
  const nodes = new Map<string, ProjectTreeNode>();
  for (const project of projects) nodes.set(project.id, { project, children: [] });
  const roots: ProjectTreeNode[] = [];
  for (const node of nodes.values()) {
    const parentId = node.project.parent_id;
    const parent = parentId ? nodes.get(parentId) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
  }
  return roots;
}

/** ノード直接所属の未読数を、自分+子孫の合算へボトムアップ集約する。 */
export function aggregateUnread(
  projects: Project[],
  direct: Record<string, number>,
): Record<string, number> {
  const result: Record<string, number> = {};
  const childrenOf = new Map<string, string[]>();
  for (const p of projects) {
    if (p.parent_id) {
      childrenOf.set(p.parent_id, [...(childrenOf.get(p.parent_id) ?? []), p.id]);
    }
  }
  const sum = (id: string): number => {
    if (id in result) return result[id];
    let total = direct[id] ?? 0;
    for (const child of childrenOf.get(id) ?? []) total += sum(child);
    result[id] = total;
    return total;
  };
  for (const p of projects) sum(p.id);
  return result;
}
```

- 型: `src/types/project.ts` に `parent_id: string | null`、`src/types/mail.ts` に `ThreadProjectRef` と `Thread.projects: ThreadProjectRef[]`
- `projectApi.ts`: `setProjectParent` / `getProjectDeleteImpact` / `getEffectiveContext` を追加、`createProject` に `parentId?` を追加（invoke 引数名は Rust command のキャメル変換に合わせる: `parentId` → `parent_id` は Tauri が自動変換するため `{ projectId, parentId }` の形）
- `projectStore.ts`:
  - `expandedIds: Set<string>`（`localStorage.getItem("pigeon.expandedProjects")` から初期化、toggle 時に保存）
  - `setProjectParent` 追加（API → `fetchProjects(accountId)`）
  - `archiveProject`/`deleteProject`/`mergeProject` を「成功後に `fetchProjects` 再取得+`directories`/`contexts`/`scanningProjects` から消えた id を削除+`selectedProjectId` が消えていれば null」に変更
  - `fetchDirectory`/`fetchProjectContext` の set 前に `get().projects.some((p) => p.id === projectId)` を確認、false なら破棄

- [ ] **Step 4: Green を確認**

Run: `pnpm test && pnpm tsc --noEmit`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src/types/project.ts src/types/mail.ts src/api/projectApi.ts src/stores/projectTree.ts src/stores/projectStore.ts src/__tests__/projectStoreTree.test.ts
git commit -m "feat(ui): 案件ツリーの構築・集約未読・構造変更後の状態整合をストアに追加"
```

---

## Task 13: サイドバーのツリー描画と階層操作

**Files:**
- Modify: `src/components/sidebar/ProjectTree.tsx` / `ProjectListItem.tsx`
- Create: `src/components/sidebar/MoveProjectDialog.tsx`
- Test: `src/__tests__/ProjectTreeNested.test.tsx`（新規）、既存 `ProjectTreeDrop.test.tsx` の追従

**Interfaces:**
- Consumes: `buildProjectTree` / `aggregateUnread` / `expandedIds` / `setProjectParent` / `getProjectDeleteImpact`（Task 12）
- Produces:
  - ProjectTree の再帰描画（インデント 16px/深さ、シェブロンで展開/折りたたみ、バッジは集約値）
  - コンテキストメニュー追加: 「＋ 子案件を作成」（既存 ProjectForm を parent 指定で開く）「親を変更...」（MoveProjectDialog）
  - 削除確認ダイアログ: `getProjectDeleteImpact` の件数と「配下のメールは未分類に戻ります。同じスレッドに他案件のメールがある場合、AIが再分類することがあります」を表示
  - `MoveProjectDialog`: props `{ projectId: string; onClose: () => void }`。ツリーピッカーで新しい親（または「ルート（親なし）」）を選択。自分と子孫は disabled
  - メール D&D のドロップ先は任意ノード（既存 `handleDropOnProject` を各ノードに配線）

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/ProjectTreeNested.test.tsx`（既存 ProjectTree テストの Provider/モック流儀に合わせる）:

```tsx
it("renders children indented under expanded parent and hides when collapsed", async () => {
  // ストアに root/mid(parent=root) を注入し、初期状態(展開済み)で mid が見える
  // シェブロンをクリックすると mid が消える
});

it("shows aggregated unread badge on parent", async () => {
  // direct: root=1, mid=2 → root のバッジ表示は 3
});

it("move dialog disables self and descendants", async () => {
  // root を移動対象にしたとき root と mid の選択肢が disabled
});
```

（コメント部は実テストコードとして書き起こす。既存テストのセットアップヘルパ——ストアのモック方法・render ラッパ——を流用し、`screen.getByText`/`fireEvent.click` で検証する）

- [ ] **Step 2: Red を確認**

Run: `pnpm test ProjectTreeNested`
Expected: FAIL

- [ ] **Step 3: 実装**

- `ProjectTree.tsx`: `projects.map(...)` のフラット描画を `buildProjectTree(projects)` の再帰描画に変更。`ProjectTreeNodeItem`（内部コンポーネント）が `depth` を受けて `paddingLeft: depth * 16`、子がある場合のみシェブロン表示、`expandedIds` で開閉
- 既存 `ProjectListItem` は行描画として再利用し、バッジ値を `aggregateUnread` の値に差し替え
- コンテキストメニュー（`getProjectMenuItems`）に「＋ 子案件を作成」「親を変更...」を追加。子作成は既存の作成フォームを `parentId` 付きで開き `createProject(..., parentId)` を呼ぶ
- `MoveProjectDialog.tsx`: ツリーを再帰描画したラジオ選択+「ルート（親なし）」。disabled 判定は「選択対象のサブツリー集合」（フロントで `buildProjectTree` から算出）
- 削除メニューのハンドラ: `getProjectDeleteImpact` を取得してから確認ダイアログを出す（文言は上記固定文）

- [ ] **Step 4: Green を確認**

Run: `pnpm test && pnpm tsc --noEmit`
Expected: PASS（既存 ProjectTree 系テストの追従修正を含む）

- [ ] **Step 5: コミット**

```bash
git add src/components/sidebar/ src/__tests__/
git commit -m "feat(ui): サイドバーを案件ツリー表示にし子案件作成・親の変更・サブツリー削除確認を追加"
```

---

## Task 14: スレッド一覧・検索・承認ダイアログの階層対応

**Files:**
- Modify: `src/components/thread-list/ThreadList.tsx`（パンくず・所属チップ）
- Modify: 検索バーコンポーネント（`src/components/` 内の既存検索UI。実ファイル名に合わせる）
- Modify: `src/components/common/NewProjectProposal.tsx`（作成位置の表示）
- Modify: コンテキスト設定画面（継承分の表示。`src/components/` 内の既存設定UI）

**Interfaces:**
- Consumes: `Thread.projects`（Task 6/12）、`searchApi` のスコープ引数（Task 11）、`getEffectiveContext`（Task 12）、`approve_new_project` の parent（Task 10）
- Produces:
  - スレッド一覧ヘッダに選択案件のパンくず（`ancestor_path` はフロントの `projects` 配列から合成——`buildProjectTree` と同じ親子表で十分）
  - 親ノード閲覧時のスレッド行に `thread.projects[].display_path` のチップ
  - 検索欄に「この案件内で検索」トグル（案件選択中のみ表示、デフォルトOFF。ON のとき `searchMails`/`semanticSearch` に `projectId` を渡す）
  - AI の子案件作成提案の承認UIに「作成先: <パス>」を表示（変更は親選択ドロップダウン=ルート含む案件一覧）
  - コンテキスト設定画面に `getEffectiveContext` の継承分を「継承: <ノード名>」ラベル付き読み取り専用で表示

- [ ] **Step 1: 失敗するテストを書く**

```tsx
it("shows relative path chips for threads from descendant projects", () => {
  // thread.projects = [{ project_id: "leaf", display_path: "埼玉 > 音響" }] を渡して
  // "埼玉 > 音響" のチップが描画される
});

it("search scope toggle passes projectId to search API", () => {
  // トグルON で searchApi.searchMails が projectId 付きで呼ばれる
});

it("new project proposal shows target path when parent is proposed", () => {
  // parent_project_id 付き提案で「作成先: ツアー」が表示される
});
```

（各テストは対象コンポーネントの既存テストファイルの流儀——モック・render ヘルパ——で実コード化する）

- [ ] **Step 2: Red を確認**

Run: `pnpm test`
Expected: FAIL

- [ ] **Step 3: 実装**

各コンポーネントへ上記 Interfaces の通り実装。検索トグルの状態は検索UIのローカル state で持ち、選択案件が変わったら維持（OFF に戻さない）。パンくず合成はストアの `projects` から親を辿る小関数を `projectTree.ts` に追加:

```typescript
/** ストア上の projects からパンくず文字列を合成（" > " 区切り、設計書の区切り規約） */
export function projectPathString(projects: Project[], id: string): string {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const parts: string[] = [];
  let cur = byId.get(id);
  while (cur) {
    parts.unshift(cur.name);
    cur = cur.parent_id ? byId.get(cur.parent_id) : undefined;
  }
  return parts.join(" > ");
}
```

- [ ] **Step 4: Green を確認**

Run: `pnpm test && pnpm tsc --noEmit`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src/components/ src/stores/projectTree.ts src/__tests__/
git commit -m "feat(ui): スレッド一覧のパンくず・所属チップ・案件内検索トグル・提案の作成先表示を追加"
```

**→ PR E を作成**（ブランチ `feat/project-hierarchy-ui`、base: PR D（または B〜D マージ後の main））

---

## 完了条件

- `cd src-tauri && cargo test` / `pnpm test` / `pnpm tsc --noEmit` が全 PASS
- 設計書 `docs/design/2026-07-18-hierarchical-projects-design.md` の §4〜§9 の各項目に対応するタスクが完了している
- 5 PR がマージされ、アプリ上で: 子案件の作成→AI分類がパスを提示→親選択で集約表示→案件内検索、の一連が動作する
