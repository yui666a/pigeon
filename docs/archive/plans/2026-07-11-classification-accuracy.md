# 分類精度の改善 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 分類器に渡す入力情報（本文文字数・案件の代表送信者・直近件名件数）を増やし、Ollama/Claude 両方の分類精度を上げる。

**Architecture:** プロンプト構築 `classifier::prompt` はプロバイダ非依存。本文プレビューを1000文字に拡張し、案件ごとに「頻度上位の送信者」を集計して `ProjectSummary` に載せ、プロンプトに反映する。確信度閾値は変更しない。

**Tech Stack:** Rust / rusqlite / SQLite、React + TypeScript（警告文言のみ）。テスト: cargo test / Vitest。

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。
- アプリケーションエラーは `AppError`。
- 本文プレビューは **1000文字**（定数 `BODY_PREVIEW_CHARS = 1000`）。マルチバイトは `chars()` ベースで切る。
- 代表送信者は頻度降順・同数時は `from_addr` 昇順で決定的に、最大5件。
- 直近件名は最大10件。
- 確信度閾値（`CONFIDENCE_AUTO_ASSIGN` / `CONFIDENCE_UNCERTAIN`）は変更しない。
- Conventional Commits（scope: classifier / db / docs / ui）。

## File Structure

- `src-tauri/src/models/classifier.rs` — `BODY_PREVIEW_CHARS` 定数、`from_mail` の1000化、`ProjectSummary.top_senders`、テスト更新
- `src-tauri/src/db/assignments.rs` — `get_top_senders` 追加
- `src-tauri/src/db/projects.rs` — `build_project_summaries` に `top_senders` と件名10件
- `src-tauri/src/classifier/prompt.rs` — `Frequent senders` 行、`SYSTEM_PROMPT` 追記、テストヘルパ/テスト更新
- `agent.md`, `docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md`, `docs/superpowers/specs/2026-07-09-project-directory-context-design.md` — 文字数記述
- `src/components/sidebar/LlmSettingsDialog.tsx` — 警告バナー文言

---

## Task 1: 本文プレビューを1000文字に拡張

**Files:**
- Modify: `src-tauri/src/models/classifier.rs`
- Test: 同ファイル内 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `pub const BODY_PREVIEW_CHARS: usize = 1000;`（`from_mail` がこれを使う）

- [ ] **Step 1: 既存テストを1000前提に更新（失敗するテスト）**

`src-tauri/src/models/classifier.rs` の `test_from_mail_truncates_body_at_300_chars` と `test_from_mail_multibyte_truncation` を次に置き換える（名前も変更）:

```rust
    #[test]
    fn test_from_mail_truncates_body_at_1000_chars() {
        let long_body = "a".repeat(1500);
        let mail = make_mail(Some(&long_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 1000);
    }

    #[test]
    fn test_from_mail_body_under_limit_kept_whole() {
        let body = "a".repeat(700);
        let mail = make_mail(Some(&body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 700);
    }

    #[test]
    fn test_from_mail_multibyte_truncation() {
        let japanese_body = "あ".repeat(1500);
        let mail = make_mail(Some(&japanese_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 1000);
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test models::classifier`
Expected: FAIL（現状300で切るため `test_from_mail_truncates_body_at_1000_chars` が1000にならない）

- [ ] **Step 3: 定数追加と from_mail 修正**

`src-tauri/src/models/classifier.rs` の先頭付近（`CONFIDENCE_UNCERTAIN` の定義の下）に追加:

```rust
/// 分類プロンプトに載せる本文プレビューの最大文字数。
pub const BODY_PREVIEW_CHARS: usize = 1000;
```

`from_mail` の `.take(300)` を `.take(BODY_PREVIEW_CHARS)` に変更:

```rust
        let body_preview = mail
            .body_text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(BODY_PREVIEW_CHARS)
            .collect();
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test models::classifier`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/models/classifier.rs
git commit -m "feat(classifier): 分類用の本文プレビューを300→1000文字に拡張"
```

---

## Task 2: get_top_senders クエリを追加

**Files:**
- Modify: `src-tauri/src/db/assignments.rs`
- Test: 同ファイル内 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `pub fn get_top_senders(conn: &Connection, project_id: &str, limit: u32) -> Result<Vec<String>, AppError>`
  - 当該案件の割り当て済みメールの `from_addr` を頻度降順・同数時は `from_addr` 昇順で、最大 `limit` 件返す。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/assignments.rs` の `#[cfg(test)] mod tests` に追加する。既存ヘルパを使う: `setup_db()`、`create_account(conn, id)`、`create_project(conn, id, account_id, name)`、`make_mail(id, account_id, subject, date) -> Mail`、`insert_mail(conn, &mail)`、`assign_mail(conn, mail_id, project_id, "ai", Some(conf))`。`make_mail` は `from_addr` を `"sender@example.com"` 固定で返すので、送信者を変えるには戻り値の `from_addr` を書き換える。この `mod tests` 内にローカルヘルパを1つ足すと簡潔:

```rust
    // 指定送信者のメールを作って project に割り当てる（get_top_senders テスト用）。
    fn assign_mail_from(conn: &Connection, id: &str, project_id: &str, from: &str) {
        let mut mail = make_mail(id, "acc1", "subj", "2026-04-13T10:00:00");
        mail.from_addr = from.to_string();
        insert_mail(conn, &mail);
        assign_mail(conn, id, project_id, "ai", Some(0.9)).unwrap();
    }

    #[test]
    fn test_get_top_senders_orders_by_frequency() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "p1", "acc1", "P1");
        assign_mail_from(&conn, "m1", "p1", "a@x.com");
        assign_mail_from(&conn, "m2", "p1", "a@x.com");
        assign_mail_from(&conn, "m3", "p1", "a@x.com");
        assign_mail_from(&conn, "m4", "p1", "b@x.com");
        assign_mail_from(&conn, "m5", "p1", "b@x.com");
        assign_mail_from(&conn, "m6", "p1", "c@x.com");

        let senders = get_top_senders(&conn, "p1", 5).unwrap();
        assert_eq!(senders, vec!["a@x.com", "b@x.com", "c@x.com"]);
    }

    #[test]
    fn test_get_top_senders_respects_limit() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "p1", "acc1", "P1");
        assign_mail_from(&conn, "m1", "p1", "a@x.com");
        assign_mail_from(&conn, "m2", "p1", "b@x.com");
        assign_mail_from(&conn, "m3", "p1", "c@x.com");
        let senders = get_top_senders(&conn, "p1", 2).unwrap();
        assert_eq!(senders.len(), 2);
    }

    #[test]
    fn test_get_top_senders_ties_broken_by_addr_asc() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "p1", "acc1", "P1");
        // 全員1通ずつ（同数）→ from_addr 昇順で安定
        assign_mail_from(&conn, "m1", "p1", "zoe@x.com");
        assign_mail_from(&conn, "m2", "p1", "amy@x.com");
        assign_mail_from(&conn, "m3", "p1", "mia@x.com");
        let senders = get_top_senders(&conn, "p1", 5).unwrap();
        assert_eq!(senders, vec!["amy@x.com", "mia@x.com", "zoe@x.com"]);
    }

    #[test]
    fn test_get_top_senders_empty_for_unassigned_project() {
        let conn = setup_db();
        let senders = get_top_senders(&conn, "no-such-project", 5).unwrap();
        assert!(senders.is_empty());
    }
```

注: `create_account` は既存テストが使っているヘルパ名。もし名前が違っていたら（例: `create_test_account`）、同ファイルの既存テストで使われている実名に合わせること。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test assignments::tests::test_get_top_senders`
Expected: FAIL（`get_top_senders` 未定義）

- [ ] **Step 3: get_top_senders を実装**

`src-tauri/src/db/assignments.rs` の `get_recent_subjects` の直後に追加:

```rust
/// 案件に割り当て済みメールの送信者(from_addr)を頻度降順で返す。
/// 同数のときは from_addr 昇順で安定させる。分類プロンプトの手がかり用。
pub fn get_top_senders(
    conn: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.from_addr
         FROM mails m
         JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.project_id = ?1
         GROUP BY m.from_addr
         ORDER BY COUNT(*) DESC, m.from_addr ASC
         LIMIT ?2",
    )?;
    let senders = stmt
        .query_map(params![project_id, limit], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(senders)
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test assignments::tests::test_get_top_senders`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/assignments.rs
git commit -m "feat(db): 案件の頻出送信者を返す get_top_senders を追加"
```

---

## Task 3: ProjectSummary に top_senders を追加し build_project_summaries を更新

**Files:**
- Modify: `src-tauri/src/models/classifier.rs`（`ProjectSummary` に `top_senders`）
- Modify: `src-tauri/src/db/projects.rs`（`build_project_summaries` で `top_senders` と件名10件）
- Test: `src-tauri/src/db/projects.rs` 内の既存テスト（あれば）に追記、なければ最小テストを追加

**Interfaces:**
- Consumes: `assignments::get_top_senders`（Task 2）
- Produces: `ProjectSummary.top_senders: Vec<String>`

- [ ] **Step 1: ProjectSummary にフィールド追加**

`src-tauri/src/models/classifier.rs` の `ProjectSummary` に `top_senders` を追加:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub recent_subjects: Vec<String>,
    pub top_senders: Vec<String>,
    pub context: Option<String>,
}
```

- [ ] **Step 2: ビルドを走らせ、壊れる箇所を洗い出す**

Run: `cd src-tauri && cargo build 2>&1 | grep -E "missing field|top_senders" | head`
Expected: `ProjectSummary` を構築している箇所（`build_project_summaries` と、`prompt.rs` のテストヘルパ `make_project`、`prompt.rs` の各テスト内のインライン構築）で「missing field top_senders」エラー。これらは Task 3/4 で埋める。まず本タスクの2ファイルを直す。

- [ ] **Step 3: build_project_summaries を更新**

`src-tauri/src/db/projects.rs` の `build_project_summaries` 内、`recent_subjects` の行を10件に変え、`get_top_senders` を追加し、`ProjectSummary` 構築に `top_senders` を足す:

```rust
        let recent_subjects = assignments::get_recent_subjects(conn, &p.id, 10).unwrap_or_default();
        let top_senders = assignments::get_top_senders(conn, &p.id, 5).unwrap_or_default();
        let context = crate::db::project_contexts::get_context(conn, &p.id)?
            .filter(|c| !for_cloud || c.allow_cloud_context)
            .and_then(|c| c.cached_context)
            .map(|c| c.chars().take(800).collect::<String>());
        summaries.push(ProjectSummary {
            id: p.id,
            name: p.name,
            description: p.description,
            recent_subjects,
            top_senders,
            context,
        });
```

- [ ] **Step 4: build_project_summaries のテストを追加/更新**

`src-tauri/src/db/projects.rs` に `build_project_summaries` のテストが既にあれば、`top_senders` が反映されることと `recent_subjects` が最大10件であることのアサーションを足す。無ければ、既存テストのDBセットアップ流儀に合わせて最小テストを追加する:

```rust
    #[test]
    fn test_build_project_summaries_includes_top_senders() {
        // 既存テストと同じ手順で account/project/mails/assignments を用意し、
        // ある project に複数送信者のメールを割り当てる。
        // build_project_summaries の結果でその project の top_senders が空でないことを確認する。
        // （セットアップは同ファイル既存テストのヘルパ/手順に合わせること）
    }
```

（既存テスト資産が薄くセットアップが重い場合、`get_top_senders` 自体は Task 2 で担保済みのため、ここは「`top_senders` フィールドが結果に含まれ、割り当て済み送信者が入る」ことの確認に留める。過剰なセットアップは避ける。）

- [ ] **Step 5: テストが通ることを確認**

Run: `cd src-tauri && cargo test db::projects`
Expected: PASS（`prompt.rs` はまだ壊れている可能性があるが、それは Task 4 で直す。ここでは `db::projects` のテストが通ればよい）

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/models/classifier.rs src-tauri/src/db/projects.rs
git commit -m "feat(classifier): ProjectSummaryに代表送信者を追加し件名を10件に"
```

---

## Task 4: プロンプトに Frequent senders を反映

**Files:**
- Modify: `src-tauri/src/classifier/prompt.rs`
- Test: 同ファイル内 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `ProjectSummary.top_senders`（Task 3）

- [ ] **Step 1: テストヘルパと既存テストを top_senders 対応に更新（コンパイルを通す前提整備）**

`src-tauri/src/classifier/prompt.rs` の `mod tests` 内 `make_project` に `top_senders` を追加:

```rust
    fn make_project(id: &str, name: &str) -> ProjectSummary {
        ProjectSummary {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(format!("Description for {}", name)),
            recent_subjects: vec!["Subject A".to_string(), "Subject B".to_string()],
            top_senders: vec![],
            context: None,
        }
    }
```

同ファイルのテスト内で `ProjectSummary { ... }` をインライン構築している箇所（`test_build_user_prompt_project_without_description` など複数）すべてに `top_senders: vec![],` を追加する。

- [ ] **Step 2: Frequent senders を検証する失敗テストを追加**

`mod tests` に追加:

```rust
    #[test]
    fn test_build_user_prompt_includes_frequent_senders() {
        let mail = make_mail();
        let projects = vec![ProjectSummary {
            id: "p1".to_string(),
            name: "Finance".to_string(),
            description: None,
            recent_subjects: vec![],
            top_senders: vec![
                "丸井 <marui@example.com>".to_string(),
                "tanaka@example.com".to_string(),
            ],
            context: None,
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);
        assert!(prompt.contains("Frequent senders:"));
        assert!(prompt.contains("marui@example.com"));
        assert!(prompt.contains("tanaka@example.com"));
    }

    #[test]
    fn test_build_user_prompt_no_frequent_senders_line_when_empty() {
        let mail = make_mail();
        let projects = vec![ProjectSummary {
            id: "p1".to_string(),
            name: "Finance".to_string(),
            description: None,
            recent_subjects: vec![],
            top_senders: vec![],
            context: None,
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);
        assert!(!prompt.contains("Frequent senders:"));
    }

    #[test]
    fn test_system_prompt_mentions_sender_signal() {
        assert!(SYSTEM_PROMPT.contains("sender"));
    }
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cd src-tauri && cargo test classifier::prompt`
Expected: FAIL（`Frequent senders:` 未出力、`SYSTEM_PROMPT` に sender 記述なし）

- [ ] **Step 4: build_user_prompt と SYSTEM_PROMPT を更新**

`build_user_prompt` の、`recent_subjects` を出力するブロックの直後（`context` 出力の前）に追加:

```rust
            if !project.top_senders.is_empty() {
                prompt.push_str(&format!(
                    "  Frequent senders: {}\n",
                    project.top_senders.join("; ")
                ));
            }
```

`SYSTEM_PROMPT` の Rules 末尾（`- Use \"unclassified\" only ...` の後）に1行追記:

```
- The sender address is a strong signal; prefer a project whose frequent senders match the email's From.
```

（`SYSTEM_PROMPT` は文字列リテラルなので、該当行を追記する形で編集する。）

- [ ] **Step 5: テストが通ることを確認**

Run: `cd src-tauri && cargo test classifier`
Expected: PASS（既存 prompt テスト＋新規テストすべて緑）

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/classifier/prompt.rs
git commit -m "feat(classifier): プロンプトに案件の頻出送信者を追加"
```

---

## Task 5: セキュリティルール・警告文言の更新（300→1000）

**Files:**
- Modify: `agent.md`
- Modify: `docs/superpowers/specs/2026-07-09-project-directory-context-design.md`
- Modify: `docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md`
- Modify: `src/components/sidebar/LlmSettingsDialog.tsx`

- [ ] **Step 1: agent.md の文字数を更新**

`agent.md` のセキュリティルール行の「本文冒頭300文字」を「本文冒頭1000文字」に変更する（該当は1箇所）。

- [ ] **Step 2: 設計書の文字数を更新**

- `docs/superpowers/specs/2026-07-09-project-directory-context-design.md` 内の「本文冒頭300文字」表記（2箇所程度）を「本文冒頭1000文字」に変更する。
- `docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md` 内の警告バナー文言「本文冒頭300文字」を「本文冒頭1000文字」に変更する。

- [ ] **Step 3: ダイアログの警告バナー文言を更新**

`src/components/sidebar/LlmSettingsDialog.tsx` の「件名・送信者・本文冒頭300文字」を「件名・送信者・本文冒頭1000文字」に変更する（`border-gray-300` などのCSSクラスは変更しないこと。対象は警告バナーの日本語文言のみ）。

- [ ] **Step 4: フロントの型チェックとテスト**

Run: `pnpm tsc --noEmit && pnpm vitest run src/__tests__/LlmSettingsDialog.test.tsx`
Expected: エラーなし・PASS（既存ダイアログテストは文言の部分一致に依存していないため影響なし。もし「300」に依存するアサーションがあれば1000に更新する）

- [ ] **Step 5: コミット**

```bash
git add agent.md docs/superpowers/specs/2026-07-09-project-directory-context-design.md docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md src/components/sidebar/LlmSettingsDialog.tsx
git commit -m "docs(classifier): LLM送信の本文文字数を300→1000に更新"
```

---

## Task 6: 全体検証と設計書ステータス更新

**Files:**
- Modify: `docs/superpowers/specs/2026-07-11-classification-accuracy-improvement-design.md`

- [ ] **Step 1: Rust 全テスト**

Run: `cd src-tauri && cargo test`
Expected: 全緑（新規 get_top_senders / prompt テスト含む）。既存の300前提テストが残って落ちていないこと。

- [ ] **Step 2: フロント検証**

Run: `pnpm tsc --noEmit && pnpm vitest run`
Expected: 全緑。（repo に lint スクリプトは無いので `pnpm lint` は実行しない。）

- [ ] **Step 3: 設計書ステータス更新**

`docs/superpowers/specs/2026-07-11-classification-accuracy-improvement-design.md` のステータス行を `承認済み（実装前）` → `実装済み` に更新。

- [ ] **Step 4: コミット**

```bash
git add docs/superpowers/specs/2026-07-11-classification-accuracy-improvement-design.md
git commit -m "docs(specs): 分類精度改善の設計書ステータスを実装済みに更新"
```
