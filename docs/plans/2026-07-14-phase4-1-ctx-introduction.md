# Phase 4-1: Ctx 導入 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 散らばった Tauri managed State（DB / SecureStore / 揮発状態）を1つの借用コンテキスト `Ctx<'a>` に束ね、既存コマンドを挙動不変で Ctx 経由に付け替える。

**Architecture:** `Ctx<'a>` は Tauri が所有する各 State への参照を保持する借用構造体。各 Tauri コマンドが注入された `State<...>` から `Ctx::new(...)` を組み立てて use case / db 層へ渡す。これは後続の Phase 4-2（UseCase trait + dispatch）が受け取る単一コンテキストの土台であり、この段階では Risk ゲートも UseCase trait も導入しない。純粋なリファクタリング。

**Tech Stack:** Rust / Tauri 2 / rusqlite / cargo test

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない（agent.md）
- アプリケーションエラーは `thiserror` の `AppError` で定義済み。use case / Ctx は `Result<T, AppError>` を返す
- Tauri commands は `Result<T, String>` ではなく既存踏襲で `Result<T, AppError>`（`AppError` は Serialize 実装済み）
- モジュール名 snake_case / 構造体 PascalCase / 関数 snake_case
- TDD: Red → Green → Refactor。新しいロジック（Ctx のアクセサ）はテストを先に書く
- `cargo fmt -- <file>` はクレート全体を整形する副作用があるため、整形はコミット直前にまとめて1回
- DB 接続は `Mutex<Connection>` 単一。`with_conn` クロージャ内で await を挟まない（`state.rs:15` の制約を Ctx でも維持）

---

## File Structure

- **Create** `src-tauri/src/context.rs` — `Ctx<'a>` 構造体とアクセサ。DB ロックヘルパ（`with_conn` / `with_conn_mut`）を委譲、SecureStore / 揮発状態への参照アクセサを提供。
- **Modify** `src-tauri/src/lib.rs` — `pub mod context;` を追加（invoke_handler は変更しない）。
- **Modify** `src-tauri/src/commands/search_commands.rs` — `search_mails` を Ctx 経由に付け替える最初の実例（read 系・最小）。
- **Modify** `src-tauri/src/commands/classify_commands.rs` — `classify_mail` を Ctx 経由に付け替える（複数 State を束ねる実例）。

**この計画のスコープ外**（Phase 4-1 では触らない）: UseCase trait、dispatch、Risk、監査ログ、承認キュー、MCP、エージェント、および send/delete/archive/flag の抽出（Phase 4-3）。残りのコマンドの Ctx 付け替えは 4-1 完了後に同じパターンで水平展開する（本計画では2コマンドでパターンを確立するに留める）。

---

## Task 1: Ctx 構造体とアクセサ

**Files:**
- Create: `src-tauri/src/context.rs`
- Modify: `src-tauri/src/lib.rs:9` (module 宣言追加)
- Test: `src-tauri/src/context.rs`（`#[cfg(test)]` モジュール内）

**前提（実コード確認済み）:** `AppError` に `internal` バリアントは無い（`error.rs` 参照）。テスト用 Ctx で SecureStore 未設定を表すエラーには既存の `AppError::Validation(String)` を使う（新バリアントは追加しない・YAGNI）。`AppError` は `rusqlite::Error` に `#[from]` があるので DB クロージャは `?` 伝播可。

**Interfaces:**
- Consumes: 既存 `DbState`（`state.rs:10`、`with_conn`/`with_conn_mut` を持つ）、`SecureStoreState`（`state.rs:34`、`.0: SecureStore`）、`PendingClassifications`（`classifier/service.rs`）、`ClassifyBatches`（`classifier/service.rs`）、`SyncLocks`（`state.rs:42`）。
- Produces:
  - `pub struct Ctx<'a>` — 各 State への参照を保持。
  - `Ctx::new(db: &'a DbState, secure_store: &'a SecureStoreState, pending: &'a PendingClassifications, batches: &'a ClassifyBatches, sync_locks: &'a SyncLocks) -> Ctx<'a>`
  - `Ctx::with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T, AppError>) -> Result<T, AppError>`
  - `Ctx::with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, AppError>) -> Result<T, AppError>`
  - `Ctx::secure_store(&self) -> &SecureStore`
  - `Ctx::pending(&self) -> &PendingClassifications`
  - `Ctx::batches(&self) -> &ClassifyBatches`
  - `Ctx::sync_locks(&self) -> &SyncLocks`

- [ ] **Step 1: module 宣言を追加**

`src-tauri/src/lib.rs` の module 宣言ブロック（1〜10行目、`pub mod` が並ぶ箇所）に `context` を追加する。`commands` の直後、アルファベット順の位置に挿入:

```rust
pub mod classifier;
pub mod commands;
pub mod context;
pub mod db;
pub mod error;
```

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/context.rs` を新規作成し、まずテストだけ書く（本体はまだ空でコンパイルを通す最小の骨組みを置く）。テストは Ctx が DB ロックを委譲し、SecureStore 以外の揮発状態参照を返せることを検証する。

`test_helpers::setup_db()`（`test_helpers.rs:17`）がマイグレーション適用済みのインメモリ DB を返し、`acc1` を挿入するのでそれを使う。`AppError` は `rusqlite::Error` に `#[from]` 実装があるため（`error.rs:7`）、DB クロージャは `?` で伝播できる。

```rust
use rusqlite::Connection;
use std::sync::Mutex;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::error::AppError;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState, SyncLocks};

pub struct Ctx<'a> {
    // Step 3 で埋める
    _marker: std::marker::PhantomData<&'a ()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[test]
    fn test_with_conn_runs_closure_against_db() {
        let (db, pending, batches, locks) = build_states();
        // SecureStore はこのテストでは使わないので、
        // secure_store を要求しない with_conn 経路のみ検証する。
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let one: i64 = ctx
            .with_conn(|conn| {
                let v: i64 = conn.query_row("SELECT 1", [], |r| r.get(0))?;
                Ok(v)
            })
            .expect("with_conn should run the closure");
        assert_eq!(one, 1);
    }

    #[test]
    fn test_sync_locks_accessor_returns_shared_state() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        assert!(ctx.sync_locks().try_begin("acc1"));
        // 同じ基盤 State を指しているので、二重開始は拒否される
        assert!(!ctx.sync_locks().try_begin("acc1"));
    }
}
```

注: 本番用 `Ctx::new` は SecureStore を要求するが、SecureStore の生成には Stronghold ファイルが要るためユニットテストでは扱いにくい。テスト専用に SecureStore を持たない `Ctx::new_for_test`（`#[cfg(test)]`）を用意し、`secure_store()` を呼ばないテストに限定する。

- [ ] **Step 3: 本体を実装**

`Ctx<'a>` を実装する。SecureStore はテストで生成困難なため、`secure_store` フィールドを `Option<&'a SecureStore>` にし、本番 `new` は `Some`、`new_for_test` は `None` を入れる。`secure_store()` アクセサは未設定時に `AppError` を返す形にせず、`&SecureStore` を返すために「未設定なら panic ではなく」— ここは設計判断: **`secure_store()` は `Result<&SecureStore, AppError>` を返す**（未設定は内部エラー）。これで `expect` を避けられる。

```rust
use rusqlite::Connection;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::error::AppError;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState, SyncLocks};

/// 全 driver（commands / 将来の MCP・agent）が共有する借用コンテキスト。
/// Tauri が所有する各 managed State への参照を束ねる。
/// この段階では依存アクセサのみを提供し、Risk ゲート等は Phase 4-4 で載せる。
pub struct Ctx<'a> {
    db: &'a DbState,
    secure_store: Option<&'a SecureStore>,
    pending: &'a PendingClassifications,
    batches: &'a ClassifyBatches,
    sync_locks: &'a SyncLocks,
}

impl<'a> Ctx<'a> {
    pub fn new(
        db: &'a DbState,
        secure_store: &'a SecureStoreState,
        pending: &'a PendingClassifications,
        batches: &'a ClassifyBatches,
        sync_locks: &'a SyncLocks,
    ) -> Self {
        Self {
            db,
            secure_store: Some(&secure_store.0),
            pending,
            batches,
            sync_locks,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(
        db: &'a DbState,
        pending: &'a PendingClassifications,
        batches: &'a ClassifyBatches,
        sync_locks: &'a SyncLocks,
    ) -> Self {
        Self {
            db,
            secure_store: None,
            pending,
            batches,
            sync_locks,
        }
    }

    /// DB 接続を借りてクロージャを実行する（`DbState::with_conn` へ委譲）。
    /// クロージャ内で await を挟まないこと（ロック保持のため）。
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.db.with_conn(f)
    }

    /// `with_conn` の可変版。
    pub fn with_conn_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.db.with_conn_mut(f)
    }

    /// SecureStore への参照。テスト用 Ctx で未設定の場合はエラー。
    pub fn secure_store(&self) -> Result<&SecureStore, AppError> {
        self.secure_store.ok_or_else(|| {
            AppError::Validation("secure store not configured in this context".into())
        })
    }

    pub fn pending(&self) -> &PendingClassifications {
        self.pending
    }

    pub fn batches(&self) -> &ClassifyBatches {
        self.batches
    }

    pub fn sync_locks(&self) -> &SyncLocks {
        self.sync_locks
    }
}
```

注: `AppError` の import は `use crate::error::AppError;` のみで足りる（`Validation` は既存バリアント）。`error.rs` の変更は不要。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib context:: -- --test-threads=4`
Expected: `test_with_conn_runs_closure_against_db` と `test_sync_locks_accessor_returns_shared_state` が PASS。

- [ ] **Step 5: クレート全体のビルドとテスト**

Run: `cd src-tauri && cargo build && cargo test --lib -- --test-threads=4`
Expected: ビルド成功、既存テスト全 PASS（Ctx 追加は既存挙動に影響しない）。

- [ ] **Step 6: 整形してコミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/context.rs src-tauri/src/lib.rs
git commit -m "refactor(context): 依存を束ねる借用Ctxを導入

散らばったTauri managed State（DB/SecureStore/揮発状態）を
1つのCtx<'a>に束ねる。この段階ではアクセサのみで挙動不変。
Phase 4-2のUseCase/dispatchが受け取る単一コンテキストの土台。"
```

---

## Task 2: search_mails を Ctx 経由に付け替え（read 系の実例）

**Files:**
- Modify: `src-tauri/src/commands/search_commands.rs:8-15`
- Test: `src-tauri/src/commands/search_commands.rs`（挙動不変のため既存の db::search テストで担保。新規テストは不要）

**Interfaces:**
- Consumes: `Ctx::new`（Task 1）、`Ctx::with_conn`。
- Produces: なし（コマンドシグネチャの外形は変わるが公開 API 名は不変）。

**この付け替えのポイント:** `search_mails` は今 `State<DbState>` だけを取る。Ctx を組むには他の State も注入が要るが、read 系は DB しか使わない。ここでは **Ctx を強制せず、DB のみ使うコマンドは `Ctx` を DB だけで構築できる補助コンストラクタ**があると冗長な注入を避けられる。→ 設計判断: Task 1 の `Ctx` に read 専用の軽量コンストラクタは足さない（YAGNI）。代わりに search はフル `Ctx` を受け取る形にし、必要な State を全て注入する。これにより「全コマンドが同じ Ctx を組む」一貫性を優先する。

- [ ] **Step 1: 変更後の姿を書く**

`search_commands.rs` を以下に置き換える。全 State を注入して `Ctx` を組み、`ctx.with_conn` を使う:

```rust
use tauri::State;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::db::search;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::state::{DbState, SecureStoreState, SyncLocks};

#[tauri::command]
pub fn search_mails(
    db: State<DbState>,
    secure_store: State<SecureStoreState>,
    pending: State<PendingClassifications>,
    batches: State<ClassifyBatches>,
    sync_locks: State<SyncLocks>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    ctx.with_conn(|conn| search::search_mails(conn, &account_id, &query, 100))
}
```

注: `#[tauri::command]` は同期関数でも複数の `State<'_, T>` 注入を許容する。State はすべて `lib.rs` の `.manage(...)` で登録済み（`lib.rs:53-59`）なので invoke_handler の変更は不要。

- [ ] **Step 2: ビルドで注入が解決することを確認**

Run: `cd src-tauri && cargo build`
Expected: ビルド成功。State 注入の型が全て manage 済みのため解決する。

- [ ] **Step 3: 既存テストで挙動不変を確認**

Run: `cd src-tauri && cargo test --lib search -- --test-threads=4`
Expected: `db::search` の既存テストが PASS（検索ロジック自体は未変更）。

- [ ] **Step 4: 整形してコミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/commands/search_commands.rs
git commit -m "refactor(context): search_mailsをCtx経由に付け替え

read系コマンドの最小実例。DBのみ使うがフルCtxを組む一貫性を優先。
挙動不変（db::searchは未変更）。"
```

---

## Task 3: classify_mail を Ctx 経由に付け替え（複数 State を束ねる実例）

**Files:**
- Modify: `src-tauri/src/commands/classify_commands.rs:21-30`
- Test: 既存の `classifier::service::classify_one` テストで担保（挙動不変）

**Interfaces:**
- Consumes: `Ctx::new`、`Ctx::with_conn`、`Ctx::secure_store`、`Ctx::pending`。
- Produces: なし。

**この付け替えのポイント:** `classify_mail` は現状 `db` / `pending` / `secure_store` の3 State を取り、`build_classifier(conn, &secure_store.0)` と `service::classify_one(&db.0, ..., &pending, ...)` を呼ぶ。Ctx 化しても classifier のビルドとサービス呼び出しはそのまま。`&db.0`（`&Mutex<Connection>`）を要求する `classify_one` に対しては、Ctx が内部の DbState 参照を渡せる必要がある。→ Ctx に `db_state()` アクセサを足すのではなく、`classify_one` が要求する `&Mutex<Connection>` は `Ctx` からは直接出さない方針。代わりに **`classify_one` 側は変更せず**、コマンドで `&db.0` を従来通り使い、Ctx は `secure_store` / `pending` の取得と `with_conn`（classifier ビルド）に使う。これにより Ctx 化の範囲を「State を束ねて渡す入口」に限定し、サービス層の内部シグネチャ（Phase 4-2 の課題）には踏み込まない。

- [ ] **Step 1: 変更後の姿を書く**

`classify_commands.rs` の `classify_mail`（21〜30行目）を置き換える。`ClassifyBatches` と `SyncLocks` も Ctx 構築のため注入する:

```rust
#[tauri::command]
pub async fn classify_mail(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    secure_store: State<'_, SecureStoreState>,
    mail_id: String,
) -> Result<ClassifyResponse, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let classifier = ctx.with_conn(|conn| build_classifier(conn, ctx.secure_store()?))?;
    service::classify_one(&db.0, classifier.as_ref(), ctx.pending(), &mail_id).await
}
```

注:
- `build_classifier` の第2引数は `&SecureStore`。`ctx.secure_store()` は `Result<&SecureStore, AppError>` なので `?` で展開する。
- import に `use crate::context::Ctx;` と `use crate::state::SyncLocks;` を追加する（`SecureStoreState` は既に import 済み `classify_commands.rs:10`）。`ClassifyBatches` も既に import 済み（`classify_commands.rs:4`）。
- `&db.0` を `classify_one` に渡すのは従来通り。await を挟むため Ctx の `with_conn`（ロック保持）とは別に扱う。`build_classifier` の呼び出しは `with_conn` 内で完結し await を挟まないので安全。

- [ ] **Step 2: ビルドを確認**

Run: `cd src-tauri && cargo build`
Expected: ビルド成功。

- [ ] **Step 3: 既存テストで挙動不変を確認**

Run: `cd src-tauri && cargo test --lib classifier -- --test-threads=4`
Expected: `classifier::service` の既存テストが全 PASS。

- [ ] **Step 4: クレート全体で最終確認**

Run: `cd src-tauri && cargo test --lib -- --test-threads=4`
Expected: 全 PASS。

- [ ] **Step 5: 整形してコミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/commands/classify_commands.rs
git commit -m "refactor(context): classify_mailをCtx経由に付け替え

複数State（DB/SecureStore/pending）を束ねる実例。
classify_oneのシグネチャは未変更（Phase 4-2の課題）。挙動不変。"
```

---

## 完了条件（Phase 4-1 の Definition of Done）

- `Ctx<'a>` が `context.rs` に存在し、`with_conn` / `with_conn_mut` / `secure_store` / `pending` / `batches` / `sync_locks` を提供する。
- `search_mails`（read 系）と `classify_mail`（複数 State 系）の2コマンドが Ctx 経由で動作し、挙動不変。
- `cargo build` と `cargo test --lib` が緑。
- 残りのコマンドの Ctx 付け替えは同じパターンで水平展開可能な状態（本計画のスコープ外）。

## Self-Review 結果

- **Spec coverage:** 本計画は設計書の Phase 4-1（Ctx 導入）のみをカバー。4-2 以降（UseCase/dispatch/Risk/gate/監査/承認キュー/MCP/agent）は明示的にスコープ外と記載済み。✅
- **Placeholder scan:** 各ステップに実コードと実コマンドを記載。`AppError::internal` の存在有無だけ実装時確認が必要な旨を Task 1 Step 3 に明記（プレースホルダではなく条件分岐）。✅
- **Type consistency:** `Ctx::new` / `new_for_test` / `with_conn` / `secure_store() -> Result<&SecureStore, AppError>` の型が Task 1〜3 で一貫。`build_classifier(conn, &SecureStore)` の第2引数型と `ctx.secure_store()?` の展開が整合。✅
