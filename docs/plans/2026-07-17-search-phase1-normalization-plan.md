# 検索強化 Phase 1（正規化層）＋スパイク 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 検索の索引・クエリ双方に文字正規化（NFKC・ひらがな→カタカナ・小文字化）を適用し、全半角/かな種/大小文字の表記揺れで検索が漏れないようにする。並行トラックとして Ruri v3 の Ollama 動作検証スパイクを行う。

**Architecture:** 正規化はオフセット対応表つきの純粋関数として新設し、`fts_mails` には正規化済みテキストを格納する。SQL トリガーでは Rust の正規化関数を呼べないためトリガー同期を廃止し、`db::fts` モジュール経由の明示同期に切り替える。スニペットは正規化オフセット対応表で原文から自前生成する（正規化テキストのスニペットだと本文がカタカナ・小文字化されて表示されるため）。

**Tech Stack:** Rust (rusqlite 0.31 / SQLite FTS5 trigram / unicode-normalization crate), 既存フロントエンドは変更なし。

**設計書:** `docs/design/2026-07-17-search-enhancement-design.md`（承認済み）

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。エラーは `crate::error::AppError` を返す
- TDD: 各タスクは失敗するテストを先に書く（Red → Green → Refactor）
- コミットは Conventional Commits 形式（`feat(search): ...` / `test(db): ...` 等）、1コミット=1意図
- マイグレーション番号は **v17**（実装時点で他ブランチに v17 が現れていたら次の空き番号に読み替え、`MIGRATIONS` 配列の末尾に追記する）
- `cargo fmt` は自分が触ったファイルだけをコミットに含める（リポジトリ全体の整形結果を混ぜない）
- 作業ブランチ: `feat/search-normalization` を `docs/search-enhancement-design` から作成（設計書PRに依存する Stacked PR。設計書PRが先にマージされたら main に rebase）
- テスト実行は `cd src-tauri && cargo test`（フロントは変更なしだが最後に `pnpm test` で無事故確認）

## ファイル構成（Track A: Phase 1 実装）

| ファイル | 役割 |
|---|---|
| Create: `src-tauri/src/search_normalize.rs` | 検索用正規化（NFKC・かな統一・小文字化）とオフセット対応表 |
| Create: `src-tauri/src/search_snippet.rs` | 正規化クエリ位置から原文スニペットを生成 |
| Create: `src-tauri/src/db/fts.rs` | fts_mails への索引書き込みの一元管理（index/remove/rebuild） |
| Modify: `src-tauri/src/lib.rs` | モジュール宣言追加 |
| Modify: `src-tauri/src/db/mod.rs` | `pub mod fts;` 追加 |
| Modify: `src-tauri/src/db/migrations.rs` | migrate_v17（トリガー廃止＋FTS再構築） |
| Modify: `src-tauri/src/db/mails.rs` | insert_mail / delete_mail に FTS 同期を追加 |
| Modify: `src-tauri/src/db/accounts.rs` | delete_account に FTS 一括削除を追加 |
| Modify: `src-tauri/src/db/search.rs` | クエリ正規化・LIKE を fts_mails 対象に変更・スニペット自前生成 |
| Modify: `src-tauri/Cargo.toml` | `unicode-normalization` 追加 |

フロントエンド（`SearchResults.tsx` 等）は変更しない。スニペットは従来と同じ「プレーンテキスト＋ `<b>` ハイライト＋ `...` 省略記号」形式を維持する。

---

## Task 1: 正規化モジュール `search_normalize`

**Files:**
- Create: `src-tauri/src/search_normalize.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod search_normalize;` を既存の mod 宣言群に追加）
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Produces:
  - `pub struct NormalizedText { pub text: String, pub offsets: Vec<usize> }`（`offsets[i]` = `text` の i 番目の char が由来する原文のバイト位置）
  - `pub fn normalize_with_offsets(input: &str) -> NormalizedText`
  - `pub fn normalize_for_search(input: &str) -> String`（`normalize_with_offsets(input).text` の別名）

- [ ] **Step 1: 依存を追加**

`src-tauri/Cargo.toml` の `[dependencies]` に追加:

```toml
unicode-normalization = "0.1"
```

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/search_normalize.rs` を新規作成し、まずテストだけ書く（本体は空実装でコンパイルを通す。`todo!()` は使わずダミー値を返すと Red が明確になる）:

```rust
//! 検索用テキスト正規化。
//! 索引時（fts_mails への書き込み）とクエリ時の両方に同じ正規化を適用することで、
//! 全角/半角・ひらがな/カタカナ・大文字/小文字の表記揺れを吸収する。
//! オフセット対応表は原文スニペット生成（search_snippet）に使う。

use unicode_normalization::UnicodeNormalization;

/// 正規化結果。`offsets[i]` は `text` の i 番目の char が
/// 原文の何バイト目の char に由来するかを表す。
pub struct NormalizedText {
    pub text: String,
    pub offsets: Vec<usize>,
}

pub fn normalize_with_offsets(input: &str) -> NormalizedText {
    // Task 1 Step 4 で実装
    NormalizedText {
        text: String::new(),
        offsets: Vec::new(),
    }
}

pub fn normalize_for_search(input: &str) -> String {
    normalize_with_offsets(input).text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_lowercase() {
        assert_eq!(normalize_for_search("SATO"), "sato");
    }

    #[test]
    fn test_fullwidth_alnum_to_halfwidth() {
        assert_eq!(normalize_for_search("ＳＡＴＯ１２３"), "sato123");
    }

    #[test]
    fn test_hiragana_to_katakana() {
        assert_eq!(normalize_for_search("さとー"), "サトー");
    }

    #[test]
    fn test_halfwidth_katakana_to_fullwidth() {
        assert_eq!(normalize_for_search("ｻﾄｰ"), "サトー");
    }

    #[test]
    fn test_halfwidth_voiced_katakana_composes() {
        // 半角の濁点つきカナは NFKC で「カ + 結合濁点」に分解されるため、
        // 合成して 1 文字に戻すこと
        assert_eq!(normalize_for_search("ﾃﾞﾊﾞｲｽ"), "デバイス");
    }

    #[test]
    fn test_mixed_text() {
        assert_eq!(
            normalize_for_search("Ｐｒｉｎｔｅｒのみつもり"),
            "printerノミツモリ"
        );
    }

    #[test]
    fn test_nfkc_expansion() {
        // ㈱ は NFKC で 3 文字に展開される
        assert_eq!(normalize_for_search("㈱サトー"), "(株)サトー");
    }

    #[test]
    fn test_empty() {
        let n = normalize_with_offsets("");
        assert_eq!(n.text, "");
        assert!(n.offsets.is_empty());
    }

    #[test]
    fn test_offsets_map_to_original_bytes() {
        // 原文: "aＢか" = a(1byte) + Ｂ(3bytes) + か(3bytes)
        // 正規化: "abカ"
        let n = normalize_with_offsets("aＢか");
        assert_eq!(n.text, "abカ");
        assert_eq!(n.offsets, vec![0, 1, 4]);
    }

    #[test]
    fn test_offsets_nfkc_expansion_shares_origin() {
        // ㈱(3bytes) → "(株)" の 3 文字はすべて原文の同じ位置に由来する
        let n = normalize_with_offsets("㈱x");
        assert_eq!(n.text, "(株)x");
        assert_eq!(n.offsets, vec![0, 0, 0, 3]);
    }

    #[test]
    fn test_offsets_composed_voiced_mark() {
        // ﾃﾞ(6bytes: ﾃ=3,ﾞ=3) → デ 1 文字。由来は ﾃ の位置
        let n = normalize_with_offsets("ﾃﾞx");
        assert_eq!(n.text, "デx");
        assert_eq!(n.offsets, vec![0, 6]);
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cd src-tauri && cargo test search_normalize -- --nocapture`
Expected: FAIL（`test_ascii_lowercase` 等が空文字を返して assert 失敗）

- [ ] **Step 4: 実装**

`normalize_with_offsets` 本体とヘルパを実装:

```rust
pub fn normalize_with_offsets(input: &str) -> NormalizedText {
    let mut text = String::with_capacity(input.len());
    let mut offsets: Vec<usize> = Vec::new();
    // 1 文字ずつ NFKC → ひらがな→カタカナ → 小文字化 の順に適用し、
    // 生成された各 char に元 char のバイト位置を対応づける。
    // 文字列全体でなく 1 文字ずつ NFKC する理由はオフセット対応を保つため。
    // 文字またぎの合成（半角濁点カナ等）は push_normalized 内で処理する。
    for (byte_pos, ch) in input.char_indices() {
        for nfkc_ch in ch.nfkc() {
            let folded = hiragana_to_katakana(nfkc_ch);
            for lower in folded.to_lowercase() {
                push_normalized(&mut text, &mut offsets, lower, byte_pos);
            }
        }
    }
    NormalizedText { text, offsets }
}

/// ひらがな→カタカナ（U+3041..U+3096, 繰り返し記号 U+309D/309E を +0x60 シフト）
fn hiragana_to_katakana(ch: char) -> char {
    match ch {
        'ぁ'..='ゖ' | 'ゝ' | 'ゞ' => char::from_u32(ch as u32 + 0x60).unwrap_or(ch),
        _ => ch,
    }
}

/// 正規化済み char を 1 つ追加する。結合濁点/半濁点（半角ｶﾞ等の NFKC 分解で
/// 現れる U+3099/U+309A）は直前の文字と NFC 合成して 1 文字に戻す。
fn push_normalized(text: &mut String, offsets: &mut Vec<usize>, ch: char, byte_pos: usize) {
    if matches!(ch, '\u{3099}' | '\u{309A}') {
        if let (Some(prev), Some(prev_off)) = (text.chars().last(), offsets.last().copied()) {
            if let Some(composed) = compose_pair(prev, ch) {
                text.pop();
                offsets.pop();
                text.push(composed);
                offsets.push(prev_off);
                return;
            }
        }
    }
    text.push(ch);
    offsets.push(byte_pos);
}

/// 2 文字を NFC 合成して 1 文字になれば返す（カ + U+3099 → ガ 等）
fn compose_pair(base: char, mark: char) -> Option<char> {
    let mut buf = String::with_capacity(8);
    buf.push(base);
    buf.push(mark);
    let mut it = buf.nfc();
    let composed = it.next()?;
    if it.next().is_none() {
        Some(composed)
    } else {
        None
    }
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cd src-tauri && cargo test search_normalize`
Expected: PASS（11 tests）

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/search_normalize.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(search): 検索用正規化関数（NFKC・かな統一・小文字化・オフセット対応表）を追加"
```

---

## Task 2: スニペット生成モジュール `search_snippet`

**Files:**
- Create: `src-tauri/src/search_snippet.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod search_snippet;` 追加）

**Interfaces:**
- Consumes: `crate::search_normalize::{normalize_with_offsets, normalize_for_search, NormalizedText}`
- Produces: `pub fn make_snippet(original: &str, query: &str) -> Option<String>`
  - `query` は未正規化でよい（内部で正規化する）。原文中にマッチが見つからなければ `None`
  - 戻り値は既存 FTS スニペットと同形式: マッチ部を `<b></b>` で囲み、前後最大 30 文字、切り詰め時は `...` を付ける

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/search_snippet.rs` を新規作成:

```rust
//! 原文ベースのスニペット生成。
//! fts_mails には正規化済みテキストを格納しているため FTS5 の snippet() は
//! カタカナ・小文字化された文字列を返してしまう。ここでは正規化オフセット
//! 対応表を使って原文からスニペットを切り出す。

use crate::search_normalize::{normalize_for_search, normalize_with_offsets};

/// マッチ位置の前後に残す最大文字数
const CONTEXT_CHARS: usize = 30;

pub fn make_snippet(original: &str, query: &str) -> Option<String> {
    // Task 2 Step 3 で実装
    let _ = (original, query);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match_is_highlighted() {
        assert_eq!(
            make_snippet("見積もりの件", "見積もり"),
            Some("<b>見積もり</b>の件".into())
        );
    }

    #[test]
    fn test_normalized_match_highlights_original_text() {
        // クエリ "sato" が原文の全角 "ＳＡＴＯ" にマッチし、原文表記のまま返る
        assert_eq!(
            make_snippet("ＳＡＴＯ商事より", "sato"),
            Some("<b>ＳＡＴＯ</b>商事より".into())
        );
    }

    #[test]
    fn test_hiragana_query_matches_katakana_text() {
        assert_eq!(
            make_snippet("サトーの端末", "さとー"),
            Some("<b>サトー</b>の端末".into())
        );
    }

    #[test]
    fn test_no_match_returns_none() {
        assert_eq!(make_snippet("こんにちは", "xyz"), None);
    }

    #[test]
    fn test_empty_query_returns_none() {
        assert_eq!(make_snippet("こんにちは", ""), None);
    }

    #[test]
    fn test_long_text_is_truncated_with_ellipsis() {
        let body = format!("{}KEYWORD{}", "あ".repeat(50), "い".repeat(50));
        let snip = make_snippet(&body, "keyword").unwrap();
        assert!(snip.starts_with("..."));
        assert!(snip.ends_with("..."));
        assert!(snip.contains("<b>KEYWORD</b>"));
        // 前後 30 文字ずつに切り詰められている
        assert!(snip.contains(&"あ".repeat(30)));
        assert!(!snip.contains(&"あ".repeat(31)));
    }

    #[test]
    fn test_short_text_no_ellipsis() {
        let snip = make_snippet("abc KEYWORD def", "keyword").unwrap();
        assert_eq!(snip, "abc <b>KEYWORD</b> def");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test search_snippet`
Expected: FAIL（`make_snippet` が常に None）

- [ ] **Step 3: 実装**

```rust
pub fn make_snippet(original: &str, query: &str) -> Option<String> {
    let norm_query = normalize_for_search(query);
    if norm_query.is_empty() {
        return None;
    }
    let norm = normalize_with_offsets(original);
    let byte_start = norm.text.find(&norm_query)?;

    // 正規化テキスト内の char 位置に変換し、オフセット対応表で原文バイト位置へ
    let char_start = norm.text[..byte_start].chars().count();
    let char_end = char_start + norm_query.chars().count();
    let orig_start = norm.offsets[char_start];
    let orig_end = if char_end < norm.offsets.len() {
        norm.offsets[char_end]
    } else {
        original.len()
    };
    // NFKC 展開（㈱→"(株)" 等）でマッチ終端が始端と同じ原文 char に
    // 対応した場合でも、最低 1 文字はハイライトする
    let orig_end = orig_end.max(next_char_boundary(original, orig_start));

    let before = &original[..orig_start];
    let matched = &original[orig_start..orig_end];
    let after = &original[orig_end..];

    let prefix: String = {
        let chars: Vec<char> = before.chars().collect();
        let start = chars.len().saturating_sub(CONTEXT_CHARS);
        chars[start..].iter().collect()
    };
    let suffix: String = after.chars().take(CONTEXT_CHARS).collect();
    let head_truncated = before.chars().count() > CONTEXT_CHARS;
    let tail_truncated = after.chars().count() > CONTEXT_CHARS;

    let mut snippet = String::new();
    if head_truncated {
        snippet.push_str("...");
    }
    snippet.push_str(&prefix);
    snippet.push_str("<b>");
    snippet.push_str(matched);
    snippet.push_str("</b>");
    snippet.push_str(&suffix);
    if tail_truncated {
        snippet.push_str("...");
    }
    Some(snippet)
}

/// byte_pos の次の char 境界（byte_pos が原文末尾なら原文長）
fn next_char_boundary(s: &str, byte_pos: usize) -> usize {
    s[byte_pos..]
        .chars()
        .next()
        .map(|c| byte_pos + c.len_utf8())
        .unwrap_or(s.len())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test search_snippet`
Expected: PASS（7 tests）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/search_snippet.rs src-tauri/src/lib.rs
git commit -m "feat(search): 正規化オフセット対応表による原文スニペット生成を追加"
```

---

## Task 3: FTS 同期の Rust 移行（db::fts モジュール＋ migration v17）

**Files:**
- Create: `src-tauri/src/db/fts.rs`
- Modify: `src-tauri/src/db/mod.rs`（`pub mod fts;` 追加）
- Modify: `src-tauri/src/db/migrations.rs`（`migrate_v17` 追加、`MIGRATIONS` 末尾に `(17, migrate_v17)`）
- Modify: `src-tauri/src/db/mails.rs:111-146`（insert_mail）、`src-tauri/src/db/mails.rs:230-238`（delete_mail）
- Modify: `src-tauri/src/db/accounts.rs:89-107`（delete_account）

**Interfaces:**
- Consumes: `crate::search_normalize::normalize_for_search`、`crate::models::mail::Mail`
- Produces（`src-tauri/src/db/fts.rs`）:
  - `pub fn index_mail(conn: &Connection, mail: &Mail) -> Result<(), AppError>`
  - `pub fn remove_mail(conn: &Connection, mail_id: &str) -> Result<(), AppError>`
  - `pub fn remove_account_mails(conn: &Connection, account_id: &str) -> Result<(), AppError>`
  - `pub fn rebuild(conn: &Connection) -> Result<usize, AppError>`（再構築した行数を返す。migration と将来の再索引で使用）

**背景（実装者向け）:** 現在 fts_mails は migration v4 の SQL トリガー（`trg_fts_mails_insert` / `trg_fts_mails_delete`）で mails と同期している。正規化は Rust 関数のため SQL トリガーでは適用できず、トリガーを廃止して Rust 側で明示同期する。mails の索引対象カラム（subject/body_text/from_addr/to_addr）を書き換える経路は `insert_mail` / `delete_mail` / `delete_account` の 3 つだけであることを確認済み（フラグ・フォルダ・uid 更新は索引対象外）。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/fts.rs` を新規作成し、スタブとテストを書く:

```rust
//! fts_mails 索引の書き込みを一元管理する。
//! v17 で SQL トリガー同期を廃止したため、mails への書き込みは必ずこの
//! モジュール経由で FTS を同期すること。現在の呼び出し元:
//! insert_mail / delete_mail / delete_account。
//! 索引には search_normalize::normalize_for_search を適用した正規化済み
//! テキストを格納する（クエリ側も同じ正規化を適用して照合する）。

use crate::error::AppError;
use crate::models::mail::Mail;
use crate::search_normalize::normalize_for_search;
use rusqlite::{params, Connection};

pub fn index_mail(conn: &Connection, mail: &Mail) -> Result<(), AppError> {
    let _ = (conn, mail);
    Ok(()) // Task 3 Step 3 で実装
}

pub fn remove_mail(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    let _ = (conn, mail_id);
    Ok(())
}

pub fn remove_account_mails(conn: &Connection, account_id: &str) -> Result<(), AppError> {
    let _ = (conn, account_id);
    Ok(())
}

pub fn rebuild(conn: &Connection) -> Result<usize, AppError> {
    let _ = conn;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{accounts, mails};
    use crate::test_helpers::{make_mail, setup_db};

    fn fts_row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM fts_mails", [], |r| r.get(0))
            .unwrap()
    }

    fn fts_subject(conn: &Connection, mail_id: &str) -> String {
        conn.query_row(
            "SELECT subject FROM fts_mails WHERE mail_id = ?1",
            [mail_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn test_no_fts_triggers_after_migrations() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'trigger' AND name LIKE 'trg_fts_mails%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "v17でFTSトリガーは廃止されているはず");
    }

    #[test]
    fn test_insert_mail_indexes_normalized_text() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "ＳＡＴＯ様 みつもり", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        assert_eq!(fts_row_count(&conn), 1);
        assert_eq!(fts_subject(&conn, "m1"), "sato様 ミツモリ");
    }

    #[test]
    fn test_insert_mail_ignored_duplicate_does_not_double_index() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        assert!(mails::insert_mail(&conn, &m).unwrap());
        // 同じ (account_id, folder, uid) は INSERT OR IGNORE で弾かれる
        let mut dup = make_mail("m2", "<m2@ex.com>", "Hello", "2026-07-17T10:00:00");
        dup.uid = m.uid;
        assert!(!mails::insert_mail(&conn, &dup).unwrap());
        assert_eq!(fts_row_count(&conn), 1);
    }

    #[test]
    fn test_delete_mail_removes_fts_row() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        mails::delete_mail(&conn, "m1").unwrap();
        assert_eq!(fts_row_count(&conn), 0);
    }

    #[test]
    fn test_delete_account_removes_fts_rows() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        accounts::delete_account(&conn, "acc1").unwrap();
        assert_eq!(fts_row_count(&conn), 0);
    }

    #[test]
    fn test_rebuild_reindexes_all_mails() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<m1@ex.com>", "ＡＢＣ", "2026-07-17T10:00:00");
        let m2 = make_mail("m2", "<m2@ex.com>", "かたかな", "2026-07-17T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();
        // 索引を壊してから rebuild で復元されることを確認
        conn.execute("DELETE FROM fts_mails", []).unwrap();
        let n = rebuild(&conn).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fts_subject(&conn, "m1"), "abc");
        assert_eq!(fts_subject(&conn, "m2"), "カタカナ");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::fts`
Expected: FAIL（トリガーがまだ存在し、正規化もされていない）

- [ ] **Step 3: db::fts 本体を実装**

```rust
pub fn index_mail(conn: &Connection, mail: &Mail) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            mail.id,
            normalize_for_search(&mail.subject),
            normalize_for_search(mail.body_text.as_deref().unwrap_or("")),
            normalize_for_search(&mail.from_addr),
            normalize_for_search(&mail.to_addr),
        ],
    )?;
    Ok(())
}

pub fn remove_mail(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM fts_mails WHERE mail_id = ?1", params![mail_id])?;
    Ok(())
}

pub fn remove_account_mails(conn: &Connection, account_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM fts_mails
         WHERE mail_id IN (SELECT id FROM mails WHERE account_id = ?1)",
        params![account_id],
    )?;
    Ok(())
}

pub fn rebuild(conn: &Connection) -> Result<usize, AppError> {
    conn.execute("DELETE FROM fts_mails", [])?;
    let mut stmt = conn.prepare(
        "SELECT id, subject, body_text, from_addr, to_addr FROM mails",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut count = 0usize;
    let mut insert = conn.prepare(
        "INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for row in rows {
        let (id, subject, body_text, from_addr, to_addr) = row?;
        insert.execute(params![
            id,
            normalize_for_search(&subject),
            normalize_for_search(body_text.as_deref().unwrap_or("")),
            normalize_for_search(&from_addr),
            normalize_for_search(&to_addr),
        ])?;
        count += 1;
    }
    Ok(count)
}
```

- [ ] **Step 4: migration v17 を追加**

`src-tauri/src/db/migrations.rs` — `migrate_v16` の後に追加し、`MIGRATIONS` 配列末尾に `(17, migrate_v17),` を追記:

```rust
/// v17: FTS 索引を正規化済みテキストで再構築し、SQL トリガー同期を廃止する。
/// 正規化（search_normalize）は Rust 関数のため SQL トリガーでは適用できない。
/// 以後の同期は db::fts 経由で行う（insert_mail / delete_mail / delete_account）。
fn migrate_v17(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS trg_fts_mails_insert;
         DROP TRIGGER IF EXISTS trg_fts_mails_delete;",
    )?;
    crate::db::fts::rebuild(conn)?;
    Ok(())
}
```

- [ ] **Step 5: 書き込み経路に FTS 同期を配線**

`src-tauri/src/db/mails.rs` — `insert_mail` の戻り値組み立て部（`Ok(affected > 0)` 相当の箇所）を変更し、挿入が実際に起きたときだけ索引する:

```rust
    let inserted = affected > 0;
    if inserted {
        crate::db::fts::index_mail(conn, mail)?;
    }
    Ok(inserted)
```

`delete_mail` — 行削除の後（`affected == 0` の NotFound チェックの後）に追加。doc コメントの「FTS はトリガーで削除される」を「FTS は db::fts::remove_mail で同期する」に更新:

```rust
    crate::db::fts::remove_mail(conn, mail_id)?;
    Ok(())
```

`src-tauri/src/db/accounts.rs` — `delete_account` のトランザクション内、`DELETE FROM mails` の**前**に追加。コメント末尾の「fts_mails は mails の DELETE トリガーで同期される」を「fts_mails は db::fts::remove_account_mails で先に削除する（v17 でトリガー廃止）」に更新:

```rust
    crate::db::fts::remove_account_mails(&tx, id)?;
```

- [ ] **Step 6: テストが通ることを確認**

Run: `cd src-tauri && cargo test`
Expected: PASS（db::fts の 6 tests に加え、既存の search / mails / accounts テストが全て通る。既存テストが FTS トリガー前提で落ちる場合は、そのテストの期待値を db::fts 経由の挙動に合わせて更新する — 挙動自体は同等のはず）

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/db/fts.rs src-tauri/src/db/mod.rs src-tauri/src/db/migrations.rs src-tauri/src/db/mails.rs src-tauri/src/db/accounts.rs
git commit -m "feat(db): FTS同期をトリガーからdb::fts経由の明示同期に移行し正規化索引で再構築(v17)"
```

---

## Task 4: 検索クエリの正規化と原文スニペット

**Files:**
- Modify: `src-tauri/src/db/search.rs`
- Test: 同ファイル `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::search_normalize::normalize_for_search`、`crate::search_snippet::make_snippet`
- Produces: `pub fn search_mails(conn, account_id, query, limit) -> Result<Vec<SearchResult>, AppError>`（シグネチャ不変。呼び出し元 `usecase/cases/search.rs` とフロントエンドは変更不要）

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/search.rs` の tests に追加:

```rust
    // --- normalization integration tests (Phase 1) ---

    #[test]
    fn test_search_halfwidth_query_matches_fullwidth_subject() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "ＳＡＴＯ商事お見積り", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();

        let results = search_mails(&conn, "acc1", "sato", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_hiragana_query_matches_katakana_text() {
        let conn = setup_db();
        let mut m = make_mail("m1", "<m1@ex.com>", "端末の件", "2026-07-17T10:00:00");
        m.body_text = Some("サトー様のプリンターについて".into());
        mails::insert_mail(&conn, &m).unwrap();

        let results = search_mails(&conn, "acc1", "さとー", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_2char_normalized_like_fallback() {
        let conn = setup_db();
        let mut m = make_mail("m1", "<m1@ex.com>", "件名", "2026-07-17T10:00:00");
        m.body_text = Some("ｻﾄｰの予算".into());
        mails::insert_mail(&conn, &m).unwrap();

        // 2 文字 → LIKE フォールバック側でも正規化照合される（ｻﾄ→サト）
        let results = search_mails(&conn, "acc1", "さと", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_snippet_shows_original_text() {
        let conn = setup_db();
        let mut m = make_mail("m1", "<m1@ex.com>", "Report", "2026-07-17T10:00:00");
        m.body_text = Some("ＳＡＴＯ商事より見積書が届きました".into());
        mails::insert_mail(&conn, &m).unwrap();

        let results = search_mails(&conn, "acc1", "sato", 50).unwrap();
        assert_eq!(results.len(), 1);
        // スニペットは正規化テキストでなく原文（全角のまま）で返る
        assert!(results[0].snippet.contains("<b>ＳＡＴＯ</b>"));
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::search`
Expected: 新規 4 テストが FAIL（クエリが正規化されず 0 件、スニペットが FTS 由来）

- [ ] **Step 3: 実装**

`search_mails`: クエリを正規化してから分岐する（`is_fts_eligible` は正規化後の文字数で判定）:

```rust
pub fn search_mails(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let norm_query = crate::search_normalize::normalize_for_search(query.trim());
    if norm_query.is_empty() {
        return Ok(Vec::new());
    }

    if is_fts_eligible(&norm_query) {
        search_fts(conn, account_id, &norm_query, limit)
    } else {
        search_like(conn, account_id, &norm_query, limit)
    }
}
```

`search_fts`: SQL から `snippet(...)` 列を外し、Rust 側でスニペット生成に置き換える:

```rust
fn search_fts(
    conn: &Connection,
    account_id: &str,
    norm_query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let safe_query = sanitize_fts_query(norm_query);

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name
         FROM fts_mails fts
         JOIN mails m ON fts.mail_id = m.id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE fts_mails MATCH ?1
           AND m.account_id = ?2
         ORDER BY rank
         LIMIT ?3",
        *MAIL_COLUMNS_PREFIXED
    ))?;

    let results = stmt
        .query_map(params![safe_query, account_id, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(MAIL_COLUMN_COUNT)?;
            let project_name: Option<String> = row.get(MAIL_COLUMN_COUNT + 1)?;
            Ok((mail, project_id, project_name))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results
        .into_iter()
        .map(|(mail, project_id, project_name)| {
            let snippet = snippet_for_mail(&mail, norm_query);
            SearchResult {
                mail,
                project_id,
                project_name,
                snippet,
            }
        })
        .collect())
}

/// 件名→本文の順でスニペットを試み、どちらにも無ければ件名をそのまま使う
/// （from/to だけにマッチした場合等のフォールバック）
fn snippet_for_mail(mail: &crate::models::mail::Mail, norm_query: &str) -> String {
    crate::search_snippet::make_snippet(&mail.subject, norm_query)
        .or_else(|| {
            mail.body_text
                .as_deref()
                .and_then(|body| crate::search_snippet::make_snippet(body, norm_query))
        })
        .unwrap_or_else(|| mail.subject.clone())
}
```

`search_like`: 照合対象を mails の原文から **fts_mails の正規化済みカラム**に変更し、スニペットは同じヘルパを使う:

```rust
fn search_like(
    conn: &Connection,
    account_id: &str,
    norm_query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let like_pattern = format!("%{}%", escape_like(norm_query));

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name
         FROM fts_mails fts
         JOIN mails m ON fts.mail_id = m.id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE m.account_id = ?1
           AND (fts.subject LIKE ?2 ESCAPE '\\' OR fts.body_text LIKE ?2 ESCAPE '\\' OR fts.from_addr LIKE ?2 ESCAPE '\\' OR fts.to_addr LIKE ?2 ESCAPE '\\')
         ORDER BY m.date DESC
         LIMIT ?3",
        *MAIL_COLUMNS_PREFIXED
    ))?;

    let results = stmt
        .query_map(params![account_id, like_pattern, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(MAIL_COLUMN_COUNT)?;
            let project_name: Option<String> = row.get(MAIL_COLUMN_COUNT + 1)?;
            Ok((mail, project_id, project_name))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results
        .into_iter()
        .map(|(mail, project_id, project_name)| {
            let snippet = snippet_for_mail(&mail, norm_query);
            SearchResult {
                mail,
                project_id,
                project_name,
                snippet,
            }
        })
        .collect())
}
```

- [ ] **Step 4: 全テストが通ることを確認**

Run: `cd src-tauri && cargo test`
Expected: PASS。既存テストで期待値の更新が必要になり得るもの:
- `test_search_2char_like_snippet` — スニペットが件名フォールバックから `<b>件名</b>` 形式に変わる（`!snippet.is_empty()` のままなら変更不要）
- `test_search_with_fts_operators_safely_handled` — "AND OR NOT" は小文字化され "and or not" で照合されるが、ヒット 0 件の期待は不変

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/search.rs
git commit -m "feat(search): クエリ正規化と原文スニペット生成で表記揺れ検索に対応"
```

---

## Task 5: 仕上げ（lint・全体確認・PR）

**Files:**
- 変更なし（検証と PR 作成のみ）

- [ ] **Step 1: lint と整形**

Run:
```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
```
Expected: warning なし。`git diff --stat` を確認し、自分が触ったファイル以外の整形差分が出ていたらコミットに含めない（`git checkout -- <file>` で戻す）

- [ ] **Step 2: 全テスト実行**

Run: `cd src-tauri && cargo test && cd .. && pnpm test`
Expected: Rust・フロントとも全 PASS（フロントは無変更の無事故確認）

- [ ] **Step 3: 動作確認（実アプリ）**

デバッグビルドで起動し、検索パネルで以下を目視確認:
1. 全角/半角違いのクエリで既存メールがヒットする
2. ひらがなクエリでカタカナ本文がヒットする
3. スニペットが原文表記（カタカナ化・小文字化されていない）で表示される

- [ ] **Step 4: PR 作成**

```bash
git push -u origin feat/search-normalization
gh pr create --base docs/search-enhancement-design \
  --title "feat(search): 検索の文字正規化（全半角・かな種・大小文字の表記揺れ対応）" \
  --body "$(cat <<'EOF'
## 概要
設計書 `docs/design/2026-07-17-search-enhancement-design.md` の Phase 1（正規化層）。

- 検索用正規化関数（NFKC・ひらがな→カタカナ・小文字化・オフセット対応表）を追加
- fts_mails を正規化済みテキストで再構築（migration v17）し、SQLトリガー同期を db::fts 経由の明示同期に移行
- クエリ側にも同じ正規化を適用（FTS・LIKEフォールバック両経路）
- スニペットはオフセット対応表で原文から自前生成（表示は原文表記のまま）

## 依存
Stacked PR: 親は設計書PR（docs/search-enhancement-design）。親マージ後に base を main に変更する。

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```
（設計書ブランチの PR が先にマージ済みなら `--base main` に読み替え、事前に `git rebase origin/main`）

---

## Track B（並行実行可）: スパイク — Ruri v3 の Ollama 動作検証

**目的:** Phase 2 のデフォルト埋め込みモデルを確定する。**コードはリポジトリに入れない**（検証スクリプトは scratchpad、結果は設計書に追記）。

**合格条件（設計書 §スパイク）:**
1. Ollama v0.31.2+ で Ruri v3 GGUF が動作する
2. sentence-transformers の出力と cos 類似度 ≥ 0.99
3. 実データ相当の文例で「サトー↔佐藤」「プリンター↔端末」が上位ヒットする

- [ ] **Step 1: Ollama バージョン確認**

Run: `ollama --version`
Expected: v0.31.2 以上（ModernBERT 対応済み）。未満なら Ollama を更新してから進める

- [ ] **Step 2: Ruri v3-130m GGUF の入手とインポート**

Hugging Face で `ruri-v3-130m gguf` を検索し q8_0 版をダウンロード（例: keisuke-miyako 氏 / Targoyle 氏のコミュニティ GGUF。無ければ llama.cpp の `convert_hf_to_gguf.py` で `cl-nagoya/ruri-v3-130m` から自前変換）。

```bash
# 作業ディレクトリは scratchpad
cat > Modelfile <<'EOF'
FROM ./ruri-v3-130m-q8_0.gguf
EOF
ollama create ruri-v3-130m -f Modelfile
```

- [ ] **Step 3: 埋め込み API の動作確認**

```bash
curl -s http://localhost:11434/api/embed \
  -d '{"model":"ruri-v3-130m","input":"検索文書: 照明の仕込み図を送ります"}' | head -c 300
```
Expected: `embeddings` 配列（512 次元）が返る。エラーになる場合はこの時点で不合格 → bge-m3 フォールバック決定

- [ ] **Step 4: 本家実装との一致検証**

Python 仮想環境で sentence-transformers 版と cos 類似度を比較（同一テキスト 5 件程度、`検索文書: ` プレフィックス込み）:

```bash
python3 -m venv .venv && .venv/bin/pip install sentence-transformers requests numpy
```

```python
# verify_ruri.py（scratchpad に置く）
import numpy as np, requests
from sentence_transformers import SentenceTransformer

texts = [
    "検索文書: 照明の仕込み図を送ります",
    "検索文書: サトー株式会社の佐藤です",
    "検索文書: プリンターの調子が悪い",
    "検索クエリ: 照明",
    "検索クエリ: さとー",
]
st = SentenceTransformer("cl-nagoya/ruri-v3-130m")
a = st.encode(texts, normalize_embeddings=True)
b = []
for t in texts:
    r = requests.post("http://localhost:11434/api/embed",
                      json={"model": "ruri-v3-130m", "input": t}).json()
    v = np.array(r["embeddings"][0]); b.append(v / np.linalg.norm(v))
for t, x, y in zip(texts, a, b):
    print(f"{float(np.dot(x, y)):.4f}  {t}")
```

Expected: 全行 0.99 以上

- [ ] **Step 5: 表記揺れ・同義語の検索性能確認**

文書 20 件程度（照明/音響/ケータリング等のメール風短文。「佐藤様」「サトー株式会社」「プリンター不調」「端末の設定」等を含める）を `検索文書: ` で埋め込み、クエリ「さとー」「佐藤」「プリンター」「端末」「照明」を `検索クエリ: ` で埋め込んで cos 類似度ランキングを出力。
Expected: 「サトー⇔佐藤」「プリンター⇔端末」が相互に上位 3 位以内に入る

- [ ] **Step 6: 結果を設計書に追記**

`docs/design/2026-07-17-search-enhancement-design.md` の「スパイク」節に結果（Ollama バージョン、使用 GGUF、cos 類似度、ランキング結果、採否判定）を追記し、デフォルトモデルを確定する。コミット:

```bash
git add docs/design/2026-07-17-search-enhancement-design.md
git commit -m "docs(design): Ruri v3スパイク結果とデフォルト埋め込みモデルの確定を追記"
```

**不合格時:** デフォルトを bge-m3（`ollama pull bge-m3`、プレフィックスなし、1024 次元）に変更して設計書へ反映。Phase 2 の計画はその前提で作成する。

---

## Phase 2 / Phase 3 について

Phase 2（sqlite-vec＋埋め込みパイプライン）と Phase 3（モード切替 UI＋スマートビュー）の実装計画は、**Track B のスパイク結果でデフォルトモデルが確定してから**別ファイルとして作成する（`docs/plans/2026-07-17-search-phase2-*.md` 以降）。本計画のスコープは Phase 1 とスパイクのみ。
