# ベクトル検索（埋め込み基盤＋セマンティック検索） 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** メール本文をチャンク化して Ollama（bge-m3）でローカル埋め込みし、sqlite-vec の KNN 検索で「プリンター↔端末」のような語彙揺れに強いセマンティック検索コマンドを提供する。

**Architecture:** チャンク化は純粋関数（`mail_chunker`）、埋め込みは trait 抽象（`embedding::Embedder`、本番 Ollama / テスト Fake）、索引は sqlite-vec の `vec0` 仮想テーブル（migration v18）。埋め込み生成はバックグラウンドワーカー（`tauri::async_runtime::spawn`、同期完了後と起動時に実行）がキュー（`embedded_at IS NULL`）を処理する。検索は ADR 0004 の dispatch バス経由の read 系 UseCase として追加する。UI（モード切替・スマートビュー）は次の設計段階で別計画。

**Tech Stack:** Rust / rusqlite 0.31 (bundled) / sqlite-vec 0.1.9（静的リンク・auto_extension 登録）/ zerocopy 0.8 / Ollama `/api/embed`（bge-m3、1024次元、L2正規化済み出力）

**設計書:** `docs/design/2026-07-17-search-enhancement-design.md`（承認済み。スパイクの結果デフォルトモデルは bge-m3 に確定済み）

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。エラーは `crate::error::AppError`（Ollama 通信は既存の `AppError::OllamaConnection` / `InvalidLlmResponse` を再利用）
- TDD: 各タスクは失敗するテストを先に書く（Red → Green）
- コミットは Conventional Commits 形式、1コミット=1意図。PRタイトル・本文に「Phase 2」等の内部フェーズ名を使わない（設計書内でのみ使う）
- マイグレーションは **v18**（実装時点で他ブランチに v18 が現れていたら次の空き番号に読み替え）。`MIGRATIONS` 配列末尾に追記
- **前提ブランチ**: PR #185 / #186 がマージ済みの main から分岐すること（#186 が `delete_mail` / `delete_account` の書き込み経路を `db::tx::with_tx` でトランザクション化しており、本計画はそこに vec 索引の削除を追記する）
- 依存追加は `sqlite-vec = "0.1.9"`（alpha 系は使わない）と `zerocopy = "0.8"`（`AsBytes` は 0.8 で `IntoBytes` に改名済み。`use zerocopy::IntoBytes;` で `.as_bytes()`）
- ベクトル次元は **1024**（bge-m3）。`vec0` の DDL に固定次元で宣言する（モデル変更時は再作成＝設計書どおり）
- Ollama API は **`POST /api/embed`**（新API、バッチ入力対応、`{"model", "input": [..]}` → `{"embeddings": [[..]]}`）。旧 `/api/embeddings` は使わない
- 埋め込み対象テキストのプレフィックスは settings から読む: `embedding_query_prefix` / `embedding_document_prefix`（デフォルト空文字。bge-m3 は不要だが将来の Ruri v3 差し替えに備えた設定面）
- `cargo fmt` は自分が触ったファイルだけをコミットに含める。既存 clippy 負債（commands/*.rs 等の11件）は範囲外
- テスト実行は `cd src-tauri && cargo test`

## PR 構成

| PR | ブランチ | 内容 | タスク |
|---|---|---|---|
| A | `feat/embedding-pipeline`（base: main） | チャンク化・sqlite-vec・埋め込みワーカー | Task 1〜4 |
| B | `feat/semantic-search`（base: PR A、Stacked） | KNN検索・UseCase・command | Task 5〜7 |

## ファイル構成

| ファイル | 役割 |
|---|---|
| Create: `src-tauri/src/mail_chunker.rs` | 引用除去・チャンク分割・件名プレフィックス（純粋関数） |
| Create: `src-tauri/src/db/vec_ext.rs` | sqlite-vec の auto_extension 登録（プロセス全体で1回） |
| Create: `src-tauri/src/db/chunks.rs` | mail_chunks / vec_chunks の CRUD（埋め込みキュー・原子的格納・削除同期） |
| Create: `src-tauri/src/embedding/mod.rs` | `Embedder` trait ＋ Ollama 実装 ＋ テスト用 Fake |
| Create: `src-tauri/src/embedding/worker.rs` | 未チャンク化メールの発見→チャンク化→バッチ埋め込みのループ |
| Create: `src-tauri/src/db/vec_search.rs` | KNN → mail_id グルーピング → SearchResult 変換 |
| Modify: `src-tauri/src/db/migrations.rs` | migrate_v18（mail_chunks ＋ vec_chunks） |
| Modify: `src-tauri/src/db/mails.rs` / `accounts.rs` | 削除経路に vec 索引の削除を追記 |
| Modify: `src-tauri/src/usecase/cases/search.rs` | `SemanticSearchUseCase` 追加・登録 |
| Modify: `src-tauri/src/commands/search_commands.rs` | `semantic_search` command（クエリ埋め込み→dispatch） |
| Modify: `src-tauri/src/commands/mail_commands.rs` / `lib.rs` | 同期後・起動時のワーカー起動、`embed-progress` イベント、command 登録 |
| Modify: `src-tauri/src/db/settings まわり` | `embedding_model` 等の設定キー（既存 `load/store_llm_settings` パターンに追記） |
| Modify: `src/api/searchApi.ts` | `semanticSearch` ラッパ（UI は次段階） |

---

## Task 1: チャンク化モジュール `mail_chunker`

**Files:**
- Create: `src-tauri/src/mail_chunker.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod mail_chunker;` 追加）

**Interfaces:**
- Produces:
  - `pub fn chunk_mail(subject: &str, body_text: Option<&str>) -> Vec<String>` — 引用除去した本文を約800文字（オーバーラップ100文字・段落境界優先）で分割し、各チャンク先頭に `件名: {subject}\n` を付与。本文が空でも件名のみのチャンクを1つ返す（空 Vec は返さない）
  - `pub fn remove_quoted_lines(body: &str) -> String`（テスト用に pub）

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/mail_chunker.rs` を新規作成（スタブ＋テスト）:

```rust
//! メール本文のチャンク化。埋め込み（ベクトル索引）の入力単位を作る。
//! - 返信引用（`>` 行・引用ヘッダ以降）は索引の重複を招くため除去する
//! - 分割は文字数ベース約800文字、段落境界（空行）優先、100文字オーバーラップ
//! - 各チャンクに件名をプレフィックスとして付与する（件名は強い検索シグナル）

/// 1チャンクの目標文字数（本文部分。プレフィックスは含まない）
pub const CHUNK_TARGET_CHARS: usize = 800;
/// 隣接チャンクとのオーバーラップ文字数
pub const CHUNK_OVERLAP_CHARS: usize = 100;
/// 段落境界を探して切り戻す最大文字数
const BREAK_LOOKBACK_CHARS: usize = 200;

pub fn remove_quoted_lines(body: &str) -> String {
    // Task 1 Step 3 で実装
    body.to_string()
}

pub fn chunk_mail(subject: &str, body_text: Option<&str>) -> Vec<String> {
    // Task 1 Step 3 で実装
    let _ = (subject, body_text);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_gt_quoted_lines() {
        let body = "了解です。\n> 前回のメール\n> の引用\n以上です。";
        assert_eq!(remove_quoted_lines(body), "了解です。\n以上です。");
    }

    #[test]
    fn test_remove_original_message_block() {
        let body = "本文です。\n----- Original Message -----\nここから下は全部引用";
        assert_eq!(remove_quoted_lines(body), "本文です。");
    }

    #[test]
    fn test_remove_on_wrote_block() {
        let body = "返信本文\nOn Thu, Jul 17, 2026 sato wrote:\n引用引用";
        assert_eq!(remove_quoted_lines(body), "返信本文");
    }

    #[test]
    fn test_short_mail_is_single_chunk_with_subject_prefix() {
        let chunks = chunk_mail("照明の件", Some("仕込み図を送ります"));
        assert_eq!(chunks, vec!["件名: 照明の件\n仕込み図を送ります".to_string()]);
    }

    #[test]
    fn test_empty_body_yields_subject_only_chunk() {
        // 本文なしでも必ず1チャンク返す（「処理済み」マーカーを兼ねるため空Vecは不可）
        assert_eq!(chunk_mail("件名だけ", None), vec!["件名: 件名だけ".to_string()]);
        assert_eq!(chunk_mail("件名だけ", Some("")), vec!["件名: 件名だけ".to_string()]);
    }

    #[test]
    fn test_long_body_splits_with_overlap() {
        let body = "あ".repeat(2000);
        let chunks = chunk_mail("長文", Some(&body));
        assert!(chunks.len() >= 2, "2000文字は複数チャンクになる");
        for c in &chunks {
            assert!(c.starts_with("件名: 長文\n"), "全チャンクに件名プレフィックス");
            let body_part = c.trim_start_matches("件名: 長文\n");
            assert!(body_part.chars().count() <= CHUNK_TARGET_CHARS);
        }
        // オーバーラップ: 隣接チャンクは末尾/先頭を共有する
        let first_body = chunks[0].trim_start_matches("件名: 長文\n");
        let second_body = chunks[1].trim_start_matches("件名: 長文\n");
        let first_tail: String = first_body
            .chars()
            .skip(first_body.chars().count() - CHUNK_OVERLAP_CHARS)
            .collect();
        assert!(second_body.starts_with(&first_tail));
    }

    #[test]
    fn test_split_prefers_paragraph_break() {
        // 750文字目に空行 → 800で機械的に切らず段落境界で切る
        let body = format!("{}\n\n{}", "い".repeat(750), "う".repeat(500));
        let chunks = chunk_mail("s", Some(&body));
        let first_body = chunks[0].trim_start_matches("件名: s\n");
        assert_eq!(first_body, "い".repeat(750));
    }

    #[test]
    fn test_termination_on_pathological_input() {
        // オーバーラップで無限ループしないこと（進行保証）
        let body = "え".repeat(CHUNK_OVERLAP_CHARS + 1);
        let chunks = chunk_mail("s", Some(&body));
        assert_eq!(chunks.len(), 1);
    }
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test mail_chunker`
Expected: FAIL（remove_quoted_lines がそのまま返す・chunk_mail が空 Vec）

- [ ] **Step 3: 実装**

```rust
pub fn remove_quoted_lines(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('>') {
            continue; // 行頭引用はスキップ
        }
        if is_quote_block_header(trimmed) {
            break; // 引用ブロックヘッダ以降は全て引用とみなし打ち切る
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// 引用ブロックの開始行か（保守的な判定。取りこぼしは検索ノイズになるだけで
/// 致命的でないため、誤除去しない側に倒す）
fn is_quote_block_header(line: &str) -> bool {
    line.starts_with("-----Original Message-----")
        || line.starts_with("----- Original Message -----")
        || (line.starts_with("On ") && line.trim_end().ends_with("wrote:"))
}

pub fn chunk_mail(subject: &str, body_text: Option<&str>) -> Vec<String> {
    let prefix = format!("件名: {}\n", subject);
    let body = body_text.map(remove_quoted_lines).unwrap_or_default();
    if body.trim().is_empty() {
        return vec![format!("件名: {}", subject)];
    }
    let chars: Vec<char> = body.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let hard_end = (start + CHUNK_TARGET_CHARS).min(chars.len());
        let end = find_break(&chars, start, hard_end);
        let piece: String = chars[start..end].iter().collect();
        chunks.push(format!("{}{}", prefix, piece.trim()));
        if end >= chars.len() {
            break;
        }
        // オーバーラップ分戻る。ただし必ず前進させる（無限ループ防止）
        let next = end.saturating_sub(CHUNK_OVERLAP_CHARS);
        start = if next > start { next } else { end };
    }
    chunks
}

/// hard_end から最大 BREAK_LOOKBACK_CHARS だけ手前に空行（段落境界）を探す。
/// 見つからなければ hard_end で切る。
fn find_break(chars: &[char], start: usize, hard_end: usize) -> usize {
    if hard_end >= chars.len() {
        return chars.len();
    }
    let floor = hard_end.saturating_sub(BREAK_LOOKBACK_CHARS).max(start + 1);
    let mut i = hard_end;
    while i > floor {
        if chars[i - 1] == '\n' && i >= 2 && chars[i - 2] == '\n' {
            return i;
        }
        i -= 1;
    }
    hard_end
}
```

`src-tauri/src/lib.rs` の mod 宣言群に `pub mod mail_chunker;` を追加。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test mail_chunker`
Expected: PASS（8 tests）

補足: `test_split_prefers_paragraph_break` の期待値は実装の切り方に依存する。空行位置 750 は hard_end=800 から 200 文字以内なので段落境界 752（`い`750文字+改行2つ）で切れ、`piece.trim()` で改行が落ちて本文は `い`×750 になる。実装がテストと食い違う場合はオフバイワンを実装側で直す（テストの意図: 「機械的800ではなく空行で切る」）。

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/mail_chunker.rs src-tauri/src/lib.rs
git commit -m "feat(search): メール本文のチャンク化（引用除去・段落境界分割・件名プレフィックス）を追加"
```

---

## Task 2: sqlite-vec 統合＋migration v18＋db::chunks

**Files:**
- Create: `src-tauri/src/db/vec_ext.rs`
- Create: `src-tauri/src/db/chunks.rs`
- Modify: `src-tauri/Cargo.toml`（sqlite-vec / zerocopy 追加）
- Modify: `src-tauri/src/db/mod.rs`（`pub mod vec_ext; pub mod chunks;`）
- Modify: `src-tauri/src/db/migrations.rs`（`migrate_v18` ＋ 配列追記。run_migrations 冒頭で `vec_ext::register()`）
- Modify: `src-tauri/src/db/mails.rs`（delete_mail に vec 削除追記）
- Modify: `src-tauri/src/db/accounts.rs`（delete_account に vec 削除追記）

**Interfaces:**
- Consumes: `crate::db::tx::with_tx`（#186 で導入済み）
- Produces（後続タスクが依存）:
  - `db::vec_ext::register()` — sqlite-vec を auto_extension 登録（`std::sync::Once` で冪等）。`run_migrations` 冒頭から呼ばれるため、本番・テストの全接続で vec0 が使える
  - `db::chunks::insert_chunks(conn, mail_id: &str, chunks: &[String]) -> Result<(), AppError>`
  - `db::chunks::mails_without_chunks(conn, limit: u32) -> Result<Vec<(String, String, Option<String>)>, AppError>`（(mail_id, subject, body_text)）
  - `db::chunks::pending_chunks(conn, limit: u32) -> Result<Vec<PendingChunk>, AppError>`（`pub struct PendingChunk { pub id: i64, pub content: String }`）
  - `db::chunks::store_embedding(conn, chunk_id: i64, embedding: &[f32]) -> Result<(), AppError>`（vec_chunks INSERT ＋ embedded_at 更新を with_tx で原子化）
  - `db::chunks::pending_totals(conn) -> Result<(u64, u64), AppError>`（(未チャンク化メール数, 未埋め込みチャンク数)。進捗表示用）
  - `db::chunks::remove_mail_vectors(conn, mail_id)` / `remove_account_vectors(conn, account_id)`

- [ ] **Step 1: 依存を追加**

`src-tauri/Cargo.toml` の `[dependencies]`:

```toml
sqlite-vec = "0.1.9"
zerocopy = "0.8"
```

- [ ] **Step 2: vec_ext を実装**（登録は前提基盤なのでテストより先。動作検証は Step 3 のテストが兼ねる）

`src-tauri/src/db/vec_ext.rs`:

```rust
//! sqlite-vec 拡張の登録。auto_extension はプロセス全体設定のため Once で1回だけ行う。
//! run_migrations の冒頭から呼ばれるので、本番（lib.rs）・テスト（setup_db）の
//! どの接続でも vec0 仮想テーブルが使える。
//! 注意: rusqlite を 0.34+ に上げる際は register_auto_extension API への
//! 書き換えが必要（sqlite-vec issue #206）。

use rusqlite::ffi::sqlite3_auto_extension;
use sqlite_vec::sqlite3_vec_init;
use std::sync::Once;

static REGISTER: Once = Once::new();

pub fn register() {
    REGISTER.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
}
```

`src-tauri/src/db/migrations.rs` の `run_migrations` 冒頭に追加:

```rust
pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    crate::db::vec_ext::register();
    apply_migrations(conn, MIGRATIONS)
}
```

（注意: auto_extension は**登録後に開いた接続**に効く。`run_migrations` は接続を開いた直後に必ず呼ばれるため、v18 の CREATE VIRTUAL TABLE 実行時点では拡張が有効になっている——初回だけは登録前に接続が開かれているため、**登録が間に合わないケースがある**。これを避けるため lib.rs の `Connection::open` の**前**にも `db::vec_ext::register()` を1行追加すること。テストの `setup_db` 系は `run_migrations` 内の register が2接続目以降に効く…では初回テスト接続で失敗する。**正解: `register()` を `Connection` を開く前に呼ぶこと**。具体的には (1) lib.rs の `Connection::open(&db_path)` の直前、(2) `test_helpers::setup_db` の `Connection::open_in_memory()` の直前、(3) migrations.rs 内テストで直接 `Connection::open_in_memory()` している箇所には、`setup_db` を使うか各テスト冒頭に `crate::db::vec_ext::register()` を足す。`run_migrations` 冒頭の register は保険として残す）

- [ ] **Step 3: 失敗するテストを書く（db::chunks）**

`src-tauri/src/db/chunks.rs` を新規作成し、スタブ＋テスト:

```rust
//! mail_chunks（チャンク本体・埋め込みキュー）と vec_chunks（sqlite-vec 索引）の CRUD。
//! 「embedded_at IS NULL = 未埋め込みキュー」として embedding::worker が消化する。
//! mails 行の削除時は remove_mail_vectors / remove_account_vectors で
//! vec_chunks を先に消すこと（mail_chunks は FK CASCADE で消えるが、
//! vec0 仮想テーブルは FK に参加しないため明示削除が必要）。

use crate::error::AppError;
use rusqlite::{params, Connection};
use zerocopy::IntoBytes;

pub struct PendingChunk {
    pub id: i64,
    pub content: String,
}

pub fn insert_chunks(conn: &Connection, mail_id: &str, chunks: &[String]) -> Result<(), AppError> {
    let _ = (conn, mail_id, chunks);
    Ok(()) // Step 5 で実装
}

pub fn mails_without_chunks(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<(String, String, Option<String>)>, AppError> {
    let _ = (conn, limit);
    Ok(Vec::new())
}

pub fn pending_chunks(conn: &Connection, limit: u32) -> Result<Vec<PendingChunk>, AppError> {
    let _ = (conn, limit);
    Ok(Vec::new())
}

pub fn store_embedding(conn: &Connection, chunk_id: i64, embedding: &[f32]) -> Result<(), AppError> {
    let _ = (conn, chunk_id, embedding);
    Ok(())
}

pub fn pending_totals(conn: &Connection) -> Result<(u64, u64), AppError> {
    let _ = conn;
    Ok((0, 0))
}

pub fn remove_mail_vectors(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    let _ = (conn, mail_id);
    Ok(())
}

pub fn remove_account_vectors(conn: &Connection, account_id: &str) -> Result<(), AppError> {
    let _ = (conn, account_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{accounts, mails};
    use crate::test_helpers::{make_mail, setup_db};

    fn vec_row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn test_v18_creates_tables_and_vec0_works() {
        let conn = setup_db();
        // vec0 仮想テーブルが機能する（拡張ロード＋DDLの検証）
        // cosine 距離はゼロベクトルで未定義のため、非ゼロのベクトルを入れる
        conn.execute(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (1, ?1)",
            params![vec![1.0f32; 1024].as_bytes()],
        )
        .unwrap();
        assert_eq!(vec_row_count(&conn), 1);
    }

    #[test]
    fn test_insert_and_list_pending_chunks() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["件名: S\nチャンク1".into(), "件名: S\nチャンク2".into()])
            .unwrap();

        let pending = pending_chunks(&conn, 10).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].content, "件名: S\nチャンク1");
        assert_eq!(pending_totals(&conn).unwrap().1, 2);
    }

    #[test]
    fn test_mails_without_chunks_excludes_chunked() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<m1@ex.com>", "S1", "2026-07-17T10:00:00");
        let m2 = make_mail("m2", "<m2@ex.com>", "S2", "2026-07-17T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();
        insert_chunks(&conn, "m1", &["c".into()]).unwrap();

        let todo = mails_without_chunks(&conn, 10).unwrap();
        assert_eq!(todo.len(), 1);
        assert_eq!(todo[0].0, "m2");
        assert_eq!(pending_totals(&conn).unwrap().0, 1);
    }

    #[test]
    fn test_store_embedding_marks_done_and_inserts_vector() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;

        store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]).unwrap();

        assert!(pending_chunks(&conn, 10).unwrap().is_empty(), "embedded_at が立つ");
        assert_eq!(vec_row_count(&conn), 1);
    }

    #[test]
    fn test_store_embedding_rolls_back_atomically() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;
        // 次元不一致の埋め込みは vec0 が拒否する → embedded_at も立たないこと
        assert!(store_embedding(&conn, chunk_id, &vec![0.5f32; 4]).is_err());
        assert_eq!(pending_chunks(&conn, 10).unwrap().len(), 1, "キューに残る");
    }

    #[test]
    fn test_delete_mail_removes_vectors_and_chunks() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;
        store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]).unwrap();

        mails::delete_mail(&conn, "m1").unwrap();

        assert_eq!(vec_row_count(&conn), 0, "vec_chunks も消える");
        let chunk_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mail_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunk_count, 0, "mail_chunks は CASCADE で消える");
    }

    #[test]
    fn test_delete_account_removes_vectors() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;
        store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]).unwrap();

        accounts::delete_account(&conn, "acc1").unwrap();
        assert_eq!(vec_row_count(&conn), 0);
    }
}
```

- [ ] **Step 4: Red を確認**

Run: `cd src-tauri && cargo test db::chunks`
Expected: FAIL（vec_chunks テーブル未作成 → `test_v18_creates_tables_and_vec0_works` が no such table）

- [ ] **Step 5: migration v18 と chunks 本体を実装**

`migrations.rs` に追加し、`MIGRATIONS` 末尾に `(18, migrate_v18),`:

```rust
/// v18: ベクトル検索用のチャンクテーブルと sqlite-vec 索引を作成する。
/// vec_chunks の次元 1024 は埋め込みモデル（bge-m3）に対応する。
/// モデル変更時は両テーブルを作り直して全再埋め込みする（設計書参照）。
fn migrate_v18(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS mail_chunks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            mail_id     TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
            chunk_index INTEGER NOT NULL,
            content     TEXT NOT NULL,
            embedded_at TEXT,
            UNIQUE(mail_id, chunk_index)
        );
        CREATE INDEX IF NOT EXISTS idx_mail_chunks_pending
            ON mail_chunks(embedded_at) WHERE embedded_at IS NULL;

        CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[1024] distance_metric=cosine
        );
        ",
    )?;
    Ok(())
}
```

`db/chunks.rs` 本体:

```rust
pub fn insert_chunks(conn: &Connection, mail_id: &str, chunks: &[String]) -> Result<(), AppError> {
    crate::db::tx::with_tx(conn, |conn| {
        let mut stmt = conn.prepare(
            "INSERT INTO mail_chunks (mail_id, chunk_index, content) VALUES (?1, ?2, ?3)",
        )?;
        for (i, content) in chunks.iter().enumerate() {
            stmt.execute(params![mail_id, i as i64, content])?;
        }
        Ok(())
    })
}

pub fn mails_without_chunks(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<(String, String, Option<String>)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.subject, m.body_text FROM mails m
         WHERE NOT EXISTS (SELECT 1 FROM mail_chunks c WHERE c.mail_id = m.id)
         ORDER BY m.date DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn pending_chunks(conn: &Connection, limit: u32) -> Result<Vec<PendingChunk>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, content FROM mail_chunks
         WHERE embedded_at IS NULL
         ORDER BY id
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(PendingChunk {
                id: r.get(0)?,
                content: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn store_embedding(conn: &Connection, chunk_id: i64, embedding: &[f32]) -> Result<(), AppError> {
    crate::db::tx::with_tx(conn, |conn| {
        conn.execute(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (?1, ?2)",
            params![chunk_id, embedding.as_bytes()],
        )?;
        conn.execute(
            "UPDATE mail_chunks SET embedded_at = datetime('now') WHERE id = ?1",
            params![chunk_id],
        )?;
        Ok(())
    })
}

pub fn pending_totals(conn: &Connection) -> Result<(u64, u64), AppError> {
    let unchunked: u64 = conn.query_row(
        "SELECT COUNT(*) FROM mails m
         WHERE NOT EXISTS (SELECT 1 FROM mail_chunks c WHERE c.mail_id = m.id)",
        [],
        |r| r.get(0),
    )?;
    let unembedded: u64 = conn.query_row(
        "SELECT COUNT(*) FROM mail_chunks WHERE embedded_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    Ok((unchunked, unembedded))
}

pub fn remove_mail_vectors(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM vec_chunks WHERE chunk_id IN
         (SELECT id FROM mail_chunks WHERE mail_id = ?1)",
        params![mail_id],
    )?;
    Ok(())
}

pub fn remove_account_vectors(conn: &Connection, account_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM vec_chunks WHERE chunk_id IN
         (SELECT c.id FROM mail_chunks c
          JOIN mails m ON c.mail_id = m.id
          WHERE m.account_id = ?1)",
        params![account_id],
    )?;
    Ok(())
}
```

削除経路への配線（#186 適用後の形が前提）:

`db/mails.rs` `delete_mail` の with_tx クロージャ内、`DELETE FROM mails` の**前**に:

```rust
        crate::db::chunks::remove_mail_vectors(conn, mail_id)?;
```

`db/accounts.rs` `delete_account` のトランザクション内、`DELETE FROM mails` の**前**（fts の remove_account_mails の隣）に:

```rust
    crate::db::chunks::remove_account_vectors(&tx, id)?;
```

lib.rs の `Connection::open(&db_path)` の直前に `db::vec_ext::register();` を、`test_helpers::setup_db` の `Connection::open_in_memory()` の直前にも同じ1行を追加。migrations.rs 内テストで raw `Connection::open_in_memory()` を使う箇所（アトミック性テスト等）にも各テスト冒頭に register を追加する。

- [ ] **Step 6: Green を確認**

Run: `cd src-tauri && cargo test db::chunks && cargo test`
Expected: 全 PASS。既存の migration テスト（v17 アップグレードパス等）が v18 で壊れていないことも確認

- [ ] **Step 7: コミット**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/db/vec_ext.rs src-tauri/src/db/chunks.rs src-tauri/src/db/mod.rs src-tauri/src/db/migrations.rs src-tauri/src/db/mails.rs src-tauri/src/db/accounts.rs src-tauri/src/lib.rs src-tauri/src/test_helpers.rs
git commit -m "feat(db): sqlite-vecによるチャンク埋め込み索引と埋め込みキューを追加(v18)"
```

---

## Task 3: Embedder trait ＋ Ollama 実装 ＋ 設定キー

**Files:**
- Create: `src-tauri/src/embedding/mod.rs`
- Modify: `src-tauri/src/lib.rs`（`pub mod embedding;`）
- （確認済み: `classifier::build_http_client` は既に `pub(crate)` で `Result<reqwest::Client, AppError>` を返す（classifier/mod.rs:20）。classifier 側の変更は不要）
- Modify: settings 集約（`load_llm_settings` / `store_llm_settings` と `LlmSettings` 型がある settings_commands.rs 周辺、`src/types/settings.ts`、`src/api/settingsApi.ts`）

**Interfaces:**
- Consumes: `crate::classifier::build_http_client()`（30秒タイムアウトの reqwest Client を `Result<reqwest::Client, AppError>` で返す）、`AppError::{OllamaConnection, InvalidLlmResponse}`、db::settings の `get_or_default` / `get_u32_or`
- Produces:
  - `#[async_trait] pub trait Embedder: Send + Sync { fn dimensions(&self) -> usize; async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError>; }`（async_trait は classifier の LLM trait と同じ流儀。Cargo.toml に無ければ追加）
  - `pub struct OllamaEmbedder { endpoint, model, dimensions, client }` / `OllamaEmbedder::from_settings(conn) -> Result<Self, AppError>`
  - `#[cfg(test)] pub struct FakeEmbedder`（決定的な埋め込み: 文字列ハッシュから生成。テスト共有のため `embedding::fake` として cfg(test) 公開）
  - settings キー: `embedding_model`（default `bge-m3`）/ `embedding_dimensions`（default `1024`）/ `embedding_query_prefix`（default 空）/ `embedding_document_prefix`（default 空）。既存 `ollama_endpoint` を共用

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/embedding/mod.rs`（スタブ＋テスト。HTTP 呼び出しの成形・応答パースを wiremock なしで検証するため、リクエスト JSON 構築とレスポンス JSON パースを純関数に切り出してテストする）:

```rust
//! 埋め込み生成の抽象。本番は Ollama /api/embed（バッチ対応・L2正規化済みを返す）。
//! モデル・次元・プレフィックスは settings で差し替え可能（設計書: モデル変更時は
//! vec_chunks を作り直して全再埋め込み）。

use crate::error::AppError;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[async_trait]
pub trait Embedder: Send + Sync {
    fn dimensions(&self) -> usize;
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError>;
}

pub struct OllamaEmbedder {
    endpoint: String,
    model: String,
    dimensions: usize,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

pub(crate) fn build_embed_request(model: &str, inputs: &[String]) -> serde_json::Value {
    json!({ "model": model, "input": inputs })
}

/// レスポンスを検証つきでパースする。件数・次元の不一致は InvalidLlmResponse。
pub(crate) fn parse_embed_response(
    body: &str,
    expected_count: usize,
    expected_dims: usize,
) -> Result<Vec<Vec<f32>>, AppError> {
    let _ = (body, expected_count, expected_dims);
    Err(AppError::InvalidLlmResponse("todo".into())) // Step 3 で実装
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_embed_request_shape() {
        let req = build_embed_request("bge-m3", &["a".into(), "b".into()]);
        assert_eq!(req["model"], "bge-m3");
        assert_eq!(req["input"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_parse_embed_response_ok() {
        let body = r#"{"embeddings": [[0.1, 0.2], [0.3, 0.4]]}"#;
        let v = parse_embed_response(body, 2, 2).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], vec![0.1, 0.2]);
    }

    #[test]
    fn test_parse_embed_response_count_mismatch_is_error() {
        let body = r#"{"embeddings": [[0.1, 0.2]]}"#;
        assert!(parse_embed_response(body, 2, 2).is_err());
    }

    #[test]
    fn test_parse_embed_response_dims_mismatch_is_error() {
        let body = r#"{"embeddings": [[0.1, 0.2, 0.3]]}"#;
        assert!(parse_embed_response(body, 1, 2).is_err());
    }

    #[test]
    fn test_parse_embed_response_invalid_json_is_error() {
        assert!(parse_embed_response("not json", 1, 2).is_err());
    }
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test embedding`
Expected: FAIL（parse が常に Err）

- [ ] **Step 3: 実装**

```rust
pub(crate) fn parse_embed_response(
    body: &str,
    expected_count: usize,
    expected_dims: usize,
) -> Result<Vec<Vec<f32>>, AppError> {
    let parsed: EmbedResponse = serde_json::from_str(body)
        .map_err(|e| AppError::InvalidLlmResponse(format!("embed response parse: {e}")))?;
    if parsed.embeddings.len() != expected_count {
        return Err(AppError::InvalidLlmResponse(format!(
            "embed count mismatch: got {}, expected {}",
            parsed.embeddings.len(),
            expected_count
        )));
    }
    for e in &parsed.embeddings {
        if e.len() != expected_dims {
            return Err(AppError::InvalidLlmResponse(format!(
                "embed dims mismatch: got {}, expected {}",
                e.len(),
                expected_dims
            )));
        }
    }
    Ok(parsed.embeddings)
}

impl OllamaEmbedder {
    /// build_http_client は Result を返す（classifier/mod.rs:20）ため new も Result
    pub fn new(endpoint: String, model: String, dimensions: usize) -> Result<Self, AppError> {
        Ok(Self {
            endpoint,
            model,
            dimensions,
            client: crate::classifier::build_http_client()?,
        })
    }

    /// settings から endpoint/model/dimensions を読んで構築する。
    /// キーと既定値: ollama_endpoint(既存) / embedding_model="bge-m3" /
    /// embedding_dimensions=1024
    pub fn from_settings(conn: &rusqlite::Connection) -> Result<Self, AppError> {
        use crate::db::settings;
        let endpoint = settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434")?;
        let model = settings::get_or_default(conn, "embedding_model", "bge-m3")?;
        let dimensions = settings::get_u32_or(conn, "embedding_dimensions", 1024)? as usize;
        Self::new(endpoint, model, dimensions)
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/api/embed", self.endpoint.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&build_embed_request(&self.model, inputs))
            .send()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(AppError::OllamaConnection(format!(
                "embed HTTP {}",
                resp.status()
            )));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;
        parse_embed_response(&body, inputs.len(), self.dimensions)
    }
}
```

テスト用 Fake（同ファイル。worker / vec_search のテストから使うため `#[cfg(test)]` で公開）:

```rust
#[cfg(test)]
pub mod fake {
    use super::*;

    /// 決定的なフェイク埋め込み。同じ入力は同じベクトル、字面が近いほど
    /// 近いベクトルにはならない（一致検索のテスト専用）。fail_always を
    /// 立てると常に OllamaConnection エラー（キュー滞留のテスト用）。
    pub struct FakeEmbedder {
        pub dims: usize,
        pub fail_always: bool,
    }

    #[async_trait]
    impl Embedder for FakeEmbedder {
        fn dimensions(&self) -> usize {
            self.dims
        }

        async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            if self.fail_always {
                return Err(AppError::OllamaConnection("fake down".into()));
            }
            Ok(inputs
                .iter()
                .map(|s| {
                    let mut v = vec![0.0f32; self.dims];
                    for (i, b) in s.bytes().enumerate() {
                        v[i % self.dims] += f32::from(b) / 255.0;
                    }
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                    v.iter_mut().for_each(|x| *x /= norm);
                    v
                })
                .collect())
        }
    }
}
```

settings への追記: `AppError` 型・関数名は実ファイルに合わせること。`async-trait` は Cargo.toml:49 に既存（追加不要）。

設定の入出力（`load_llm_settings` / `store_llm_settings` と `LlmSettings` struct、`src/types/settings.ts` の型、`settingsApi.ts`）に `embeddingModel: string` を追加する（serde rename は既存フィールドの流儀に合わせる。dimensions / prefix はバックエンド既定値のみで v1 は UI 非公開のため型追加不要）。

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test embedding && cargo test && pnpm tsc --noEmit 2>/dev/null || pnpm test`
Expected: Rust 全 PASS、フロントは型変更のみで既存テスト PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/embedding src-tauri/src/lib.rs src-tauri/src/commands/settings_commands.rs src/types/settings.ts src/api/settingsApi.ts src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(search): Embedder抽象とOllama埋め込みクライアント・埋め込みモデル設定を追加"
```

（変更したファイルが上記と異なる場合は実際のファイルに読み替えて明示 add）

---

## Task 4: 埋め込みワーカーと起動・同期後フック

**Files:**
- Create: `src-tauri/src/embedding/worker.rs`（`embedding/mod.rs` に `pub mod worker;`）
- Modify: `src-tauri/src/commands/mail_commands.rs`（同期成功後に spawn、`embed-progress` イベント）
- Modify: `src-tauri/src/lib.rs`（起動時 spawn。既存の起動時スキャン spawn（lib.rs:141 付近）と同型）

**Interfaces:**
- Consumes: `mail_chunker::chunk_mail` / `db::chunks::*` / `embedding::Embedder` / 設定キー `embedding_document_prefix`
- Produces:
  - `pub async fn run_embedding_pass(db: &DbState, embedder: &dyn Embedder, doc_prefix: &str, on_progress: &mut (dyn FnMut(u64, u64) + Send)) -> Result<u64, AppError>`
    - 戻り値 = 埋め込んだチャンク数。Ollama 接続エラー時は**エラーにせずそこで打ち切って Ok を返す**（キューに残す。次回パスで再開）
    - DB State は `state::DbState`（`std::sync::Mutex<Connection>`、state.rs:10）で確認済み。ロック取得は `DbState::with_conn` を使う（クロージャ内に await を入れられないため、ロックを await をまたいで保持しない構造が自然に守られる。`embedder.embed().await` は必ず `with_conn` の外で呼ぶ）
  - Tauri イベント `embed-progress`: `{ done: u64, total: u64 }`（`SyncProgressEvent` と同型の Serialize struct）
  - 多重起動ガード: `AtomicBool`（State に追加）。走行中なら新しいパスは即 return

- [ ] **Step 1: 失敗するテストを書く**

`worker.rs` のテスト（DB は `setup_db` の in-memory を `DbState` に包む。FakeEmbedder 使用。テスト内の `unwrap` は可）:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{chunks, mails};
    use crate::embedding::fake::FakeEmbedder;
    use crate::state::DbState;
    use crate::test_helpers::{make_mail, setup_db};

    fn db() -> DbState {
        DbState(std::sync::Mutex::new(setup_db()))
    }

    #[tokio::test]
    async fn test_pass_chunks_and_embeds_all_mails() {
        let db = db();
        {
            let conn = db.0.lock().unwrap();
            let mut m = make_mail("m1", "<m1@ex.com>", "照明の件", "2026-07-17T10:00:00");
            m.body_text = Some("仕込み図を送ります".into());
            mails::insert_mail(&conn, &m).unwrap();
        }
        let embedder = FakeEmbedder { dims: 1024, fail_always: false };
        let mut calls = Vec::new();
        let done = run_embedding_pass(&db, &embedder, "", &mut |d, t| calls.push((d, t)))
            .await
            .unwrap();
        assert_eq!(done, 1);
        let conn = db.0.lock().unwrap();
        assert_eq!(chunks::pending_totals(&conn).unwrap(), (0, 0));
        assert!(!calls.is_empty(), "進捗コールバックが呼ばれる");
    }

    #[tokio::test]
    async fn test_pass_leaves_queue_when_embedder_down() {
        let db = db();
        {
            let conn = db.0.lock().unwrap();
            mails::insert_mail(&conn, &make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00"))
                .unwrap();
        }
        let embedder = FakeEmbedder { dims: 1024, fail_always: true };
        let done = run_embedding_pass(&db, &embedder, "", &mut |_, _| {}).await.unwrap();
        assert_eq!(done, 0, "接続エラーはOkで打ち切り");
        let conn = db.0.lock().unwrap();
        let (_, unembedded) = chunks::pending_totals(&conn).unwrap();
        assert!(unembedded >= 1, "チャンク化は済みキューに残る");
    }

    #[tokio::test]
    async fn test_pass_applies_document_prefix() {
        // doc_prefix はチャンク本文の前に付けて埋め込み入力にする
        // （FakeEmbedder は入力文字列で決まるので、prefix 有無でベクトルが変わること
        //  を直接検証するのは難しい。ここでは build_inputs 純関数を切り出して検証）
        let inputs = build_embed_inputs("検索文書: ", &["件名: S\n本文".to_string()]);
        assert_eq!(inputs, vec!["検索文書: 件名: S\n本文".to_string()]);
    }
}
```

（`#[tokio::test]` が使えるか確認。dev-dependencies に tokio の test feature が無ければ追加。既存の async テストの流儀があればそれに従う）

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test embedding::worker`
Expected: FAIL（run_embedding_pass 未実装でコンパイルエラー→スタブを置いて assert 失敗まで持っていく）

- [ ] **Step 3: 実装**

```rust
//! 埋め込みキューの消化ワーカー。
//! 「未チャンク化メールをチャンク化 → 未埋め込みチャンクをバッチ埋め込み」を
//! キューが空になるまで繰り返す。Ollama 停止中は静かに打ち切り、次回の
//! パス（次の同期後 or 次回起動時）で自然に再開する。
//! DB ロックは with_conn 単位で取得・解放し、await をまたいで保持しない。
//! 接続エラー以外のエラー（次元不一致等）はパス全体を Err で打ち切る。
//! 同じチャンクが恒常的に失敗するとキューが進まなくなる制限がある
//! （「既知の制限」参照。v1 はモデル・次元が固定のため許容）。

use crate::db::chunks;
use crate::embedding::Embedder;
use crate::error::AppError;
use crate::mail_chunker::chunk_mail;
use crate::state::DbState;

const CHUNKING_BATCH: u32 = 100;
const EMBED_BATCH: u32 = 16;

pub(crate) fn build_embed_inputs(doc_prefix: &str, contents: &[String]) -> Vec<String> {
    contents.iter().map(|c| format!("{doc_prefix}{c}")).collect()
}

pub async fn run_embedding_pass(
    db: &DbState,
    embedder: &dyn Embedder,
    doc_prefix: &str,
    on_progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<u64, AppError> {
    // 1. チャンク化: 未チャンク化メールが尽きるまで
    loop {
        let batch = db.with_conn(|conn| chunks::mails_without_chunks(conn, CHUNKING_BATCH))?;
        if batch.is_empty() {
            break;
        }
        db.with_conn(|conn| {
            for (mail_id, subject, body) in &batch {
                let pieces = chunk_mail(subject, body.as_deref());
                chunks::insert_chunks(conn, mail_id, &pieces)?;
            }
            Ok(())
        })?;
    }

    // 2. 埋め込み: キューが尽きるか接続エラーまで
    let total = db.with_conn(chunks::pending_totals)?.1;
    let mut done: u64 = 0;
    loop {
        let batch = db.with_conn(|conn| chunks::pending_chunks(conn, EMBED_BATCH))?;
        if batch.is_empty() {
            break;
        }
        let contents: Vec<String> = batch.iter().map(|c| c.content.clone()).collect();
        let inputs = build_embed_inputs(doc_prefix, &contents);
        // embed の await は with_conn（＝ロック）の外で行う
        let embeddings = match embedder.embed(&inputs).await {
            Ok(e) => e,
            // 接続エラーは「今は埋め込めない」だけ。キューに残して静かに終了
            Err(AppError::OllamaConnection(_)) => break,
            Err(e) => return Err(e),
        };
        db.with_conn(|conn| {
            for (chunk, embedding) in batch.iter().zip(embeddings.iter()) {
                chunks::store_embedding(conn, chunk.id, embedding)?;
            }
            Ok(())
        })?;
        done += batch.len() as u64;
        on_progress(done, total);
    }
    Ok(done)
}
```

配線（コード形は既存の起動時スキャン・SyncProgressEvent と同型にする）:

1. `lib.rs` 起動時: 既存の起動時スキャン spawn の後に、`OllamaEmbedder::from_settings` → `run_embedding_pass` を `tauri::async_runtime::spawn`。`from_settings` や pass 内のエラーは `eprintln!` ログのみ（起動を阻害しない）
2. `mail_commands.rs` `sync_account_locked`: 同期成功後に同じ spawn を1回。`on_progress` で `app.emit("embed-progress", EmbedProgressEvent { done, total })`
3. 多重起動ガード: アプリ State に `embedding_running: Arc<AtomicBool>` を追加し、spawn 冒頭で `compare_exchange(false, true)` に失敗したら即 return、終了時に false へ戻す
4. `DbState` は `std::sync::Mutex<Connection>`＋`with_conn` ヘルパ（state.rs:10-22、確認済み）。worker は上記のとおり `with_conn` 単位でロックを取り、`embed().await` はロックの外で呼ぶ

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test embedding && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット（2コミット）**

```bash
git add src-tauri/src/embedding/worker.rs src-tauri/src/embedding/mod.rs
git commit -m "feat(search): 埋め込みキューを消化するバックグラウンドワーカーを追加"
git add src-tauri/src/lib.rs src-tauri/src/commands/mail_commands.rs
git commit -m "feat(search): 起動時と同期完了後に埋め込みパスを起動しembed-progressを通知"
```

**→ ここで PR A を作成**（タイトル例: `feat(search): メールのチャンク埋め込みパイプライン（sqlite-vec＋Ollama）を追加`。本文に設計書参照・v18・bge-m3・「検索コマンドは子PR」を明記）

---

## Task 5: セマンティック検索の DB 層 `db::vec_search`

**Files:**
- Create: `src-tauri/src/db/vec_search.rs`
- Modify: `src-tauri/src/db/mod.rs`

**Interfaces:**
- Consumes: vec_chunks / mail_chunks（Task 2）、`SearchResult`（`models::mail`、既存検索と同じ戻り型）
- Produces:
  - `pub struct ChunkHit { pub chunk_id: i64, pub mail_id: String, pub content: String, pub distance: f64 }`
  - `pub fn search_chunks(conn, query_embedding: &[f32], k: u32) -> Result<Vec<ChunkHit>, AppError>` — **RAG-ready の内部層**（設計書の「検索 API の層分け」）。KNN → mail_chunks JOIN でチャンク列を距離昇順に返す。将来の RAG はこの関数を入口にする
  - `pub fn search_mails_semantic(conn, account_id: &str, query_embedding: &[f32], limit: u32) -> Result<Vec<SearchResult>, AppError>` — UI 層。`search_chunks`（k = limit×4、上限200）の結果を mail_id ごとに最良距離で集約し、mails/projects を JOIN して関連度順の SearchResult にする
  - snippet はベストチャンクの本文（`件名: …\n` プレフィックスを除去し先頭120文字）。文字列マッチではないため `<b>` ハイライトなし（プレーンテキスト。フロントの DOMPurify はそのまま通す）

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{chunks, mails};
    use crate::test_helpers::{make_mail, setup_db};

    /// 単純な直交ベクトルで「近いチャンクを持つメールが上位」を検証する
    fn axis_vec(axis: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[axis] = 1.0;
        v
    }

    fn insert_mail_with_embedded_chunk(
        conn: &rusqlite::Connection,
        mail_id: &str,
        subject: &str,
        content: &str,
        axis: usize,
    ) {
        let m = make_mail(mail_id, &format!("<{mail_id}@ex.com>"), subject, "2026-07-17T10:00:00");
        mails::insert_mail(conn, &m).unwrap();
        chunks::insert_chunks(conn, mail_id, &[content.to_string()]).unwrap();
        let id = chunks::pending_chunks(conn, 100).unwrap().last().unwrap().id;
        chunks::store_embedding(conn, id, &axis_vec(axis)).unwrap();
    }

    #[test]
    fn test_semantic_search_ranks_by_distance() {
        let conn = setup_db();
        insert_mail_with_embedded_chunk(&conn, "m1", "照明", "件名: 照明\n灯体の件", 0);
        insert_mail_with_embedded_chunk(&conn, "m2", "音響", "件名: 音響\nスピーカー", 1);

        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].mail.id, "m1", "クエリに近い軸のメールが1位");
    }

    #[test]
    fn test_semantic_search_groups_chunks_per_mail() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        chunks::insert_chunks(&conn, "m1", &["c1".into(), "c2".into()]).unwrap();
        for c in chunks::pending_chunks(&conn, 10).unwrap() {
            chunks::store_embedding(&conn, c.id, &axis_vec(0)).unwrap();
        }
        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results.len(), 1, "同一メールの複数チャンクは1件に集約");
    }

    #[test]
    fn test_semantic_search_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'O', 'o@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        insert_mail_with_embedded_chunk(&conn, "m1", "S1", "c", 0);
        let mut m2 = make_mail("m2", "<m2@ex.com>", "S2", "2026-07-17T11:00:00");
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m2).unwrap();
        chunks::insert_chunks(&conn, "m2", &["c".into()]).unwrap();
        let id = chunks::pending_chunks(&conn, 10).unwrap()[0].id;
        chunks::store_embedding(&conn, id, &axis_vec(0)).unwrap();

        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_semantic_snippet_strips_subject_prefix() {
        let conn = setup_db();
        insert_mail_with_embedded_chunk(&conn, "m1", "照明", "件名: 照明\n灯体を3台追加します", 0);
        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results[0].snippet, "灯体を3台追加します");
    }

    #[test]
    fn test_semantic_search_empty_index_returns_empty() {
        let conn = setup_db();
        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_chunks_returns_chunk_hits_in_distance_order() {
        // RAG-ready 内部層: チャンク単位の結果が距離昇順で返る
        let conn = setup_db();
        insert_mail_with_embedded_chunk(&conn, "m1", "照明", "件名: 照明\n灯体の件", 0);
        insert_mail_with_embedded_chunk(&conn, "m2", "音響", "件名: 音響\nスピーカー", 1);

        let hits = search_chunks(&conn, &axis_vec(0), 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].mail_id, "m1");
        assert!(hits[0].distance <= hits[1].distance);
        assert!(hits[0].content.contains("灯体"));
    }
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::vec_search`
Expected: FAIL（未実装）

- [ ] **Step 3: 実装**

```rust
//! セマンティック検索の DB 層。KNN（vec_chunks）→ mail_id 集約 → SearchResult。
//! 戻り型を既存の文字列検索（db::search）と同じ SearchResult に揃えることで、
//! フロントエンドは検索モードによらず同じ表示コードを使える。

use crate::db::mails::{row_to_mail, MAIL_COLUMNS_PREFIXED, MAIL_COLUMN_COUNT};
use crate::error::AppError;
use crate::models::mail::SearchResult;
use rusqlite::{params, Connection};
use zerocopy::IntoBytes;

/// KNN の k はメール集約で目減りするため limit より広めに取る。
/// account_id フィルタは KNN の後段で適用されるため、複数アカウント環境では
/// 上位 k を他アカウントが占有すると取りこぼしが起きる（「既知の制限」参照）
const KNN_FACTOR: u32 = 4;
const KNN_MAX: u32 = 200;
const SNIPPET_MAX_CHARS: usize = 120;

pub struct ChunkHit {
    pub chunk_id: i64,
    pub mail_id: String,
    pub content: String,
    pub distance: f64,
}

/// RAG-ready の内部層: クエリベクトルに近いチャンクを距離昇順で返す。
/// 将来の RAG（要約・質問応答）はこの関数を入口にする（設計書「検索 API の層分け」）。
pub fn search_chunks(
    conn: &Connection,
    query_embedding: &[f32],
    k: u32,
) -> Result<Vec<ChunkHit>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.mail_id, c.content, knn.distance
         FROM (SELECT chunk_id, distance FROM vec_chunks
               WHERE embedding MATCH ?1 AND k = ?2
               ORDER BY distance) knn
         JOIN mail_chunks c ON c.id = knn.chunk_id
         ORDER BY knn.distance",
    )?;
    let hits = stmt
        .query_map(params![query_embedding.as_bytes(), k], |row| {
            Ok(ChunkHit {
                chunk_id: row.get(0)?,
                mail_id: row.get(1)?,
                content: row.get(2)?,
                distance: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(hits)
}

pub fn search_mails_semantic(
    conn: &Connection,
    account_id: &str,
    query_embedding: &[f32],
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let k = (limit * KNN_FACTOR).min(KNN_MAX);
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name, c.content, knn.distance
         FROM (SELECT chunk_id, distance FROM vec_chunks
               WHERE embedding MATCH ?1 AND k = ?2
               ORDER BY distance) knn
         JOIN mail_chunks c ON c.id = knn.chunk_id
         JOIN mails m ON m.id = c.mail_id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE m.account_id = ?3
         ORDER BY knn.distance",
        *MAIL_COLUMNS_PREFIXED
    ))?;

    let rows = stmt
        .query_map(
            params![query_embedding.as_bytes(), k, account_id],
            |row| {
                let mail = row_to_mail(row)?;
                let project_id: Option<String> = row.get(MAIL_COLUMN_COUNT)?;
                let project_name: Option<String> = row.get(MAIL_COLUMN_COUNT + 1)?;
                let content: String = row.get(MAIL_COLUMN_COUNT + 2)?;
                let distance: f64 = row.get(MAIL_COLUMN_COUNT + 3)?;
                Ok((mail, project_id, project_name, content, distance))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // distance 昇順で来るので、初出の mail_id だけ採用すれば「メールごとの
    // ベストチャンク」になる。複数案件に割り当てられたメールは LEFT JOIN で
    // 行が増え、最初の行の案件が採用される（既存 db::search::search_mails と同挙動）
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();
    for (mail, project_id, project_name, content, _distance) in rows {
        if !seen.insert(mail.id.clone()) {
            continue;
        }
        let snippet = chunk_snippet(&content);
        results.push(SearchResult {
            mail,
            project_id,
            project_name,
            snippet,
        });
        if results.len() as u32 >= limit {
            break;
        }
    }
    Ok(results)
}

/// チャンク本文からスニペットを作る。`件名: …\n` プレフィックスを除去し、
/// 先頭 SNIPPET_MAX_CHARS 文字に切り詰める（セマンティック一致のため
/// `<b>` ハイライトは付けない）
fn chunk_snippet(content: &str) -> String {
    let body = match content.split_once('\n') {
        Some((first, rest)) if first.starts_with("件名: ") => rest,
        _ => content,
    };
    let mut s: String = body.chars().take(SNIPPET_MAX_CHARS).collect();
    if body.chars().count() > SNIPPET_MAX_CHARS {
        s.push_str("...");
    }
    s
}
```

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test db::vec_search && cargo test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/vec_search.rs src-tauri/src/db/mod.rs
git commit -m "feat(search): KNNとmail_id集約によるセマンティック検索DB層を追加"
```

---

## Task 6: UseCase・command・APIラッパ

**Files:**
- Modify: `src-tauri/src/usecase/cases/search.rs`（`SemanticSearchUseCase` 追加、`register_read_cases` に登録）
- Modify: `src-tauri/src/commands/search_commands.rs`（`semantic_search` command）
- Modify: `src-tauri/src/lib.rs`（invoke_handler に `semantic_search` 追加）
- Modify: `src/api/searchApi.ts`（`semanticSearch(accountId, query)` ラッパ。UI 配線は次段階）

**Interfaces:**
- Consumes: `db::vec_search::search_mails_semantic` / `OllamaEmbedder::from_settings` / settings キー `embedding_query_prefix` / dispatch バス（`SearchMailsUseCase` と同型）
- Produces:
  - UseCase 名 `"semantic_search_mails"`、Input `{ account_id: String, embedding: Vec<f32> }`、Output `Vec<SearchResult>`、`Risk::Read`
  - Tauri command `semantic_search(account_id: String, query: String) -> Result<Vec<SearchResult>, String>` — **クエリの埋め込み生成（async HTTP）は command 側で行い、dispatch にはベクトルを渡す**（現行の dispatch は同期 run のため。DB 読みは必ずバス経由、という ADR 0004 の境界は保たれる。この判断は commit メッセージにも記す）
  - フロント: `searchApi.semanticSearch(accountId, query): Promise<SearchResult[]>`

- [ ] **Step 1: 失敗するテストを書く**

`usecase/cases/search.rs` の既存 `SearchMailsUseCase` テストと同型で:

```rust
    #[test]
    fn test_semantic_search_usecase_returns_results_via_dispatch() {
        // 既存の SearchMailsUseCase の dispatch テストと同じ組み立てで、
        // 事前に mail+チャンク+埋め込み（axis ベクトル）を挿入し、
        // dispatch(&registry, "semantic_search_mails",
        //          json!({"account_id": "acc1", "embedding": axis_vec(0)}), &ctx)
        // が該当メールを返すことを検証する。
        // Ctx / registry の組み立ては同ファイルの既存テストを踏襲すること。
    }

    #[test]
    fn test_semantic_search_usecase_is_read_risk() {
        // SemanticSearchUseCase::risk() == Risk::Read
    }
```

（既存テストの Ctx 構築ヘルパをそのまま使い、実データで検証する。モックで自己完結させない）

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test usecase::cases::search`
Expected: FAIL

- [ ] **Step 3: 実装**

`SemanticSearchUseCase`（`SearchMailsUseCase` と同じ構造）:

```rust
pub struct SemanticSearchUseCase;

#[derive(serde::Deserialize)]
pub struct SemanticSearchInput {
    pub account_id: String,
    pub embedding: Vec<f32>,
}

// impl UseCase: name() = "semantic_search_mails", risk() = Risk::Read,
// run() = ctx.with_conn(|conn| db::vec_search::search_mails_semantic(
//     conn, &input.account_id, &input.embedding, 100))
// limit 100 は既存 search_mails と同じ固定値。
// register_read_cases に registry.register(Box::new(SemanticSearchUseCase)) を追加。
```

`search_commands.rs`（既存 `search_mails` command と同型＋前段の埋め込み）:

```rust
#[tauri::command]
pub async fn semantic_search(
    /* 既存 search_mails と同じ State 群 */
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    // 1. settings から embedder とクエリプレフィックスを構築（DBロックは短く）
    // 2. embedder.embed(&[format!("{prefix}{query}")]).await → 1本のベクトル
    //    （Ollama 未起動は AppError::OllamaConnection → String でフロントへ）
    // 3. Ctx::new(...) → dispatch(&registry, "semantic_search_mails",
    //        json!({"account_id": account_id, "embedding": embedding}), &ctx)
    // 4. serde_json::from_value で Vec<SearchResult> に戻して返す
}
```

`lib.rs` invoke_handler に `semantic_search` を追加。`src/api/searchApi.ts` に:

```typescript
export async function semanticSearch(
  accountId: string,
  query: string
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("semantic_search", { accountId, query });
}
```

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test && pnpm test`
Expected: 全 PASS（command 本体は Ollama 依存のため自動テスト対象外。UseCase テストが dispatch 経由の実データ検証を担う）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/usecase/cases/search.rs src-tauri/src/commands/search_commands.rs src-tauri/src/lib.rs src/api/searchApi.ts
git commit -m "feat(search): semantic_searchコマンドをdispatchバス経由で追加"
```

---

## Task 7: 仕上げ（lint・全テスト・実機確認・PR B）

- [ ] **Step 1: lint と整形**

Run: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings 2>&1 | grep -v "commands/\|usecase/dispatch.rs"`
Expected: 今回の変更ファイルにエラーなし（既存負債11件は除外して確認）。`git diff --stat` で無関係ファイルの整形が混ざっていたら外す

- [ ] **Step 2: 全テスト**

Run: `cd src-tauri && cargo test && cd .. && pnpm test`
Expected: 全 PASS

- [ ] **Step 3: 実機確認（デバッグビルド）**

`pnpm tauri build --debug` → `open src-tauri/target/debug/bundle/macos/Pigeon.app`

1. `ollama pull bge-m3` 済みの状態で起動 → バックフィルが走る（`embed-progress` イベント／ログ、mail_chunks・vec_chunks が埋まる）
2. `sqlite3` で DB を開き `SELECT COUNT(*) FROM vec_chunks;` が増えていること
3. devtools コンソール等から `invoke("semantic_search", {accountId, query: "プリンター"})` を呼び、「端末」「デバイス」を含むメールが返ることを確認（UI は次段階のため invoke 直叩きでよい）
4. Ollama を停止して起動 → エラーなく動き、キューが残ること

- [ ] **Step 4: PR 作成**

PR A（未作成ならここで）と PR B を作成。PR B タイトル例: `feat(search): 意味で探せるセマンティック検索コマンドを追加（bge-m3）`。本文に「UI（検索モード切替・スマートビュー）は次の計画で実装」と明記。Stacked（base = PR A）。

---

## 実装順序とレビュー観点（コントローラ向けメモ）

- Task 2 は auto_extension の登録タイミング（接続を開く**前**）が唯一の罠。テストが最初に落ちたらまずそこを疑う
- Task 4 の spawn 配線は lib.rs / mail_commands.rs の既存 State 実型に合わせる箇所が多く、実装者の裁量が最も大きい。レビューでは「DBロックを await をまたいで保持していないか」「多重起動ガード」「Ollama 停止時に静かに滞留するか」を重点確認
- Task 6 は「埋め込みは command・DB読みはバス」という境界の明示がレビュー観点
- モデル差し替え（vec_chunks 再作成・全再埋め込み）の実装は本計画のスコープ外（設定キーだけ用意）。設計書の将来拡張に従い、必要になった時に別PR

## 既知の制限（v1 で許容するもの。PR 本文にも記載すること）

- **非接続系エラーで埋め込みキューが停滞し得る**: `run_embedding_pass` は `OllamaConnection` 以外のエラー（例: `embedding_dimensions` 設定と vec_chunks の固定次元 1024 の食い違いによる次元不一致）をパス全体の `Err` として返し、spawn 側はログ出力のみ。`pending_chunks` は `ORDER BY id` で毎回同じ先頭バッチを返すため、特定チャンクが恒常的に失敗すると毎パス同じ位置で中断しキューが進まない。v1 はモデル・次元設定を UI 非公開・既定値固定とするため許容する。モデル差し替えを実装する将来PRで、attempts カウンタ等の poison チャンク退避を併せて入れること
- **複数アカウント環境での KNN 取りこぼし**: `search_mails_semantic` は KNN（k ≤ 200）の後に `account_id` でフィルタするため、上位 k チャンクを他アカウントが占有すると該当アカウントの結果が痩せる・0件になり得る。複数アカウントの本格運用時に、k の動的拡大（全チャンク数比）か sqlite-vec の partition key の利用を別PRで検討する
- **複数案件に割り当てられたメールの案件名が実質不定**: `LEFT JOIN mail_project_assignments` の増殖行のうち最初の行が採用される。既存 `db::search::search_mails` と同挙動のため許容

## 次の計画（本計画完了後）

検索モード切替 UI（文字列/ベクトルのトグル・永続化）とスマートビュー（保存検索・サイドバー別セクション）。設計書の「Phase 3」に相当。`semanticSearch` API ラッパまでは本計画で用意済み。
