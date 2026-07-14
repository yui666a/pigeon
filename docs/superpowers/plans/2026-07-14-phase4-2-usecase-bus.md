# Phase 4-2: UseCase バス + Risk ゲート骨格 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 3 driver が共有する dispatch バスの骨格（UseCase trait + レジストリ + Risk ゲート + 監査シンク）を作り、read 系 UseCase を 1 つ載せてパターンを確立する。

**Architecture:** 型安全なジェネリクス `UseCase` trait（関連型 Input/Output・同期）と、`impl<T: UseCase> ErasedUseCase for T` のブランケット実装で自動導出する消去層 `ErasedUseCase`（`serde_json::Value` 境界・`dyn` 可）の 2 層構造。レジストリは `HashMap<&str, Box<dyn ErasedUseCase>>`。`dispatch` が lookup → risk → gate → audit呼び出し点 → run を一元化する。read 系のみ Risk::Read で載せ、Reversible/Sensitive はゲートが明示的に塞ぐ。

**Tech Stack:** Rust / Tauri 2 / rusqlite / serde / serde_json / cargo test

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない（agent.md）
- アプリケーションエラーは `thiserror` の `AppError`（error.rs）。新バリアントは追加しない。JSON パース失敗・未登録名・ゲート拒否は全て `AppError::Validation(String)` に載せる
- モジュール名 snake_case / 構造体・enum PascalCase / 関数 snake_case
- TDD: Red → Green → Refactor。新しいロジックはテストを先に書く
- `cargo fmt` はクレート全体を整形する。main は fmt クリーン（4-1 の 0b9b803 で整形済み）なので各コミット前に普通に `cargo fmt` してよい
- テストは `cargo test --lib -- --test-threads=4`
- DB 接続は `Mutex<Connection>` 単一。`with_conn` クロージャ内で await を挟まない
- `run` は同期（async 化は 4-5）。この計画では async を一切導入しない
- コミット/push はユーザーの明示指示を待つ（各 Task の commit ステップは指示後に実行）

---

## File Structure

新規モジュールは `src-tauri/src/usecase/` に置く（commands と横並びのアプリケーション層）。責務ごとに 1 ファイル。

- **Create** `src-tauri/src/usecase/mod.rs` — サブモジュール宣言と再エクスポート
- **Create** `src-tauri/src/usecase/risk.rs` — `Risk` enum
- **Create** `src-tauri/src/usecase/driver.rs` — `Driver` enum
- **Create** `src-tauri/src/usecase/traits.rs` — `UseCase` trait + `ErasedUseCase` + ブランケット実装
- **Create** `src-tauri/src/usecase/registry.rs` — `Registry`
- **Create** `src-tauri/src/usecase/gate.rs` — `gate::check`
- **Create** `src-tauri/src/usecase/audit.rs` — `AuditEntry` / `AuditSink` / `NoOpAuditSink` / `InMemoryAuditSink`
- **Create** `src-tauri/src/usecase/dispatch.rs` — `dispatch` 関数
- **Create** `src-tauri/src/usecase/cases/mod.rs` — cases サブモジュール宣言
- **Create** `src-tauri/src/usecase/cases/search.rs` — `SearchMailsUseCase`（read 系の実例）
- **Modify** `src-tauri/src/lib.rs:11` — `pub mod usecase;` を module 宣言ブロックに追加
- **Modify** `src-tauri/src/context.rs` — `driver` フィールドと `driver()` / `audit()` アクセサ、フェーズコメント修正

**依存順（各 Task が前の Task の型に依存する）:** risk → driver → audit → traits → registry → gate → dispatch → search UseCase → context 整合。

**audit シンクの Ctx 注入方法（設計書の未確定事項を確定）:** `Ctx::new` の引数は変えない（既存 2 command への波及を避ける）。`Ctx::audit()` は `&'static NoOpAuditSink`（`const` で持つ static 参照）を返す既定実装にする。read 系は audit を発火しないため 4-2 ではこれで足りる。SQLite シンクへの差し替えは 4-4 で Ctx にフィールドを足して行う。

---

## Task 1: usecase モジュールの骨組みと Risk enum

**Files:**
- Create: `src-tauri/src/usecase/mod.rs`
- Create: `src-tauri/src/usecase/risk.rs`
- Modify: `src-tauri/src/lib.rs:11`（module 宣言追加）
- Test: `src-tauri/src/usecase/risk.rs`（`#[cfg(test)]`）

**Interfaces:**
- Produces:
  - `pub enum Risk { Read, Reversible, Sensitive }`（`Debug, Clone, Copy, PartialEq, Eq` 導出）
  - `pub mod usecase`（lib.rs から見える）

- [ ] **Step 1: module 宣言を追加**

`src-tauri/src/lib.rs` の module 宣言ブロック（1〜11 行目、`pub mod` が並ぶ箇所）の末尾、`threading` の後に `usecase` を追加（アルファベット順）:

```rust
pub mod threading;
pub mod usecase;
```

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/usecase/risk.rs` を新規作成し、まずテストだけ書く（本体は空でコンパイルを通す最小の骨組み）:

```rust
// 本体は Step 3 で埋める

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_variants_are_distinct() {
        assert_ne!(Risk::Read, Risk::Reversible);
        assert_ne!(Risk::Reversible, Risk::Sensitive);
        assert_ne!(Risk::Read, Risk::Sensitive);
    }

    #[test]
    fn test_risk_is_copy() {
        let r = Risk::Read;
        let a = r;
        let b = r; // Copy なので r は move されない
        assert_eq!(a, b);
    }
}
```

`src-tauri/src/usecase/mod.rs` を新規作成:

```rust
pub mod risk;

pub use risk::Risk;
```

- [ ] **Step 3: 本体を実装**

`src-tauri/src/usecase/risk.rs` の先頭（tests モジュールの前）に追加:

```rust
/// 操作の危険度分類。UseCase が宣言し、dispatch のゲートが一元的に判定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// 自由に実行してよい（検索・一覧）。
    Read,
    /// 自動実行 + 監査（フラグ・未読戻し・案件移動）。ゲート実装は Phase 4-4。
    Reversible,
    /// 人間の承認必須（送信・サーバー削除）。ゲート実装は Phase 4-4。
    Sensitive,
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::risk -- --test-threads=4`
Expected: `test_risk_variants_are_distinct` と `test_risk_is_copy` が PASS。

- [ ] **Step 5: クレート全体のビルド**

Run: `cd src-tauri && cargo build`
Expected: ビルド成功。

- [ ] **Step 6: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/mod.rs src-tauri/src/usecase/risk.rs src-tauri/src/lib.rs
git commit -m "feat(usecase): Risk enumとusecaseモジュール骨組みを追加

Read/Reversible/SensitiveのRisk分類。dispatchバスの土台。"
```

---

## Task 2: Driver enum

**Files:**
- Create: `src-tauri/src/usecase/driver.rs`
- Modify: `src-tauri/src/usecase/mod.rs`（宣言 + 再エクスポート追加）
- Test: `src-tauri/src/usecase/driver.rs`（`#[cfg(test)]`）

**Interfaces:**
- Produces:
  - `pub enum Driver { Ui, Mcp, Agent }`（`Debug, Clone, Copy, PartialEq, Eq` 導出）

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/driver.rs` を新規作成:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_variants_are_distinct() {
        assert_ne!(Driver::Ui, Driver::Mcp);
        assert_ne!(Driver::Mcp, Driver::Agent);
        assert_ne!(Driver::Ui, Driver::Agent);
    }

    #[test]
    fn test_driver_is_copy() {
        let d = Driver::Ui;
        let a = d;
        let b = d;
        assert_eq!(a, b);
    }
}
```

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod driver;
pub mod risk;

pub use driver::Driver;
pub use risk::Risk;
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/driver.rs` の先頭に追加:

```rust
/// 操作の呼び出し元。ゲートの判定材料になる（分岐の中身は Phase 5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// 人間の UI 操作（承認済み扱い）。commands はすべてこれ。
    Ui,
    /// 外部 LLM（MCP 経由）。Phase 5-1。
    Mcp,
    /// 常駐エージェント。Phase 5-3。
    Agent,
}
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::driver -- --test-threads=4`
Expected: `test_driver_variants_are_distinct` と `test_driver_is_copy` が PASS。

- [ ] **Step 4: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/driver.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): Driver enum（Ui/Mcp/Agent）を追加

ゲートの判定材料。4-2ではUi固定、分岐の中身はPhase 5。"
```

---

## Task 3: AuditEntry / AuditSink / NoOpAuditSink / InMemoryAuditSink

**Files:**
- Create: `src-tauri/src/usecase/audit.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Test: `src-tauri/src/usecase/audit.rs`（`#[cfg(test)]`）

**Interfaces:**
- Consumes: `Risk`（Task 1）、`Driver`（Task 2）
- Produces:
  - `pub struct AuditEntry { pub use_case: String, pub risk: Risk, pub driver: Driver }`
  - `AuditEntry::new(use_case: &str, risk: Risk, driver: Driver) -> AuditEntry`
  - `pub trait AuditSink: Send + Sync { fn record(&self, entry: AuditEntry); }`
  - `pub struct NoOpAuditSink;`（`AuditSink` 実装・record は捨てる）
  - `pub struct InMemoryAuditSink`（`Mutex<Vec<AuditEntry>>` を内包、`new()` / `entries() -> Vec<AuditEntry>`）

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/audit.rs` を新規作成:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::{Driver, Risk};

    #[test]
    fn test_noop_sink_discards() {
        let sink = NoOpAuditSink;
        // panic せず捨てるだけ
        sink.record(AuditEntry::new("x", Risk::Reversible, Driver::Ui));
    }

    #[test]
    fn test_in_memory_sink_accumulates() {
        let sink = InMemoryAuditSink::new();
        sink.record(AuditEntry::new("send_mail", Risk::Sensitive, Driver::Agent));
        sink.record(AuditEntry::new("move_mail", Risk::Reversible, Driver::Ui));

        let entries = sink.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].use_case, "send_mail");
        assert_eq!(entries[0].risk, Risk::Sensitive);
        assert_eq!(entries[0].driver, Driver::Agent);
        assert_eq!(entries[1].use_case, "move_mail");
    }
}
```

`src-tauri/src/usecase/mod.rs` に追加（既存の宣言・再エクスポートに足す）:

```rust
pub mod audit;
pub mod driver;
pub mod risk;

pub use audit::{AuditEntry, AuditSink, InMemoryAuditSink, NoOpAuditSink};
pub use driver::Driver;
pub use risk::Risk;
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/audit.rs` の先頭（tests の前）に追加:

```rust
use std::sync::Mutex;

use crate::usecase::{Driver, Risk};

/// 監査ログの 1 エントリ。4-2 では use_case / risk / driver のみ。
/// timestamp と input 概要は 4-4 の SQLite スキーマ確定時に足す。
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub use_case: String,
    pub risk: Risk,
    pub driver: Driver,
}

impl AuditEntry {
    pub fn new(use_case: &str, risk: Risk, driver: Driver) -> Self {
        Self {
            use_case: use_case.to_string(),
            risk,
            driver,
        }
    }
}

/// 監査ログのシンク。dispatch が Reversible/Sensitive の実行時に record する。
/// 4-2 の実体は NoOp / InMemory のみ。SQLite シンクは 4-4。
pub trait AuditSink: Send + Sync {
    fn record(&self, entry: AuditEntry);
}

/// 記録を捨てる既定シンク（4-2 の read 系は監査対象外）。
pub struct NoOpAuditSink;

impl AuditSink for NoOpAuditSink {
    fn record(&self, _entry: AuditEntry) {}
}

/// テスト用: record を蓄積するシンク（4-4 の SQLite 実装の差し替え先）。
pub struct InMemoryAuditSink {
    entries: Mutex<Vec<AuditEntry>>,
}

impl InMemoryAuditSink {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// 蓄積されたエントリのスナップショット。ロック毒化時は空を返す（安全側）。
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }
}

impl Default for InMemoryAuditSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink for InMemoryAuditSink {
    fn record(&self, entry: AuditEntry) {
        if let Ok(mut v) = self.entries.lock() {
            v.push(entry);
        }
    }
}
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::audit -- --test-threads=4`
Expected: `test_noop_sink_discards` と `test_in_memory_sink_accumulates` が PASS。

- [ ] **Step 4: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/audit.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): AuditSink traitとNoOp/InMemory実装を追加

監査ログの呼び出し境界。SQLite永続化は4-4。"
```

---

## Task 4: UseCase trait + ErasedUseCase + ブランケット実装

**Files:**
- Create: `src-tauri/src/usecase/traits.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Test: `src-tauri/src/usecase/traits.rs`（`#[cfg(test)]`）

**Interfaces:**
- Consumes: `Risk`（Task 1）、`Ctx`（context.rs）、`AppError`（error.rs）
- Produces:
  - `pub trait UseCase { type Input: DeserializeOwned; type Output: Serialize; fn name(&self) -> &'static str; fn risk(&self, input: &Self::Input) -> Risk; fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError>; }`
  - `pub trait ErasedUseCase { fn name(&self) -> &str; fn risk_json(&self, input: &Value) -> Result<Risk, AppError>; fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError>; }`
  - `impl<T: UseCase> ErasedUseCase for T`（ブランケット）

**注（実コード確認済み）:** `AppError::Validation(String)` は既存バリアント（error.rs）。`Ctx` は `context.rs` にあり `with_conn` を持つ。テストでは `Ctx::new_for_test`（`#[cfg(test)]`）を使う。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/traits.rs` を新規作成。テストはダミー UseCase を定義し、ブランケットで得た ErasedUseCase 経由で Value ラウンドトリップと不正 JSON エラーを検証する:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::Risk;

    #[derive(Deserialize)]
    struct EchoInput {
        text: String,
    }

    #[derive(Serialize)]
    struct EchoOutput {
        echoed: String,
    }

    struct EchoUseCase;

    impl UseCase for EchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;

        fn name(&self) -> &'static str {
            "echo"
        }

        fn risk(&self, _input: &Self::Input) -> Risk {
            Risk::Read
        }

        fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(EchoOutput {
                echoed: input.text,
            })
        }
    }

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[test]
    fn test_erased_run_json_roundtrips() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;

        let out = uc
            .run_json(json!({ "text": "hi" }), &ctx)
            .expect("run_json should succeed");
        assert_eq!(out, json!({ "echoed": "hi" }));
    }

    #[test]
    fn test_erased_name_delegates() {
        let uc = EchoUseCase;
        assert_eq!(ErasedUseCase::name(&uc), "echo");
    }

    #[test]
    fn test_erased_risk_json_reads_input() {
        let uc = EchoUseCase;
        let risk = uc
            .risk_json(&json!({ "text": "hi" }))
            .expect("risk_json should parse input");
        assert_eq!(risk, Risk::Read);
    }

    #[test]
    fn test_erased_run_json_rejects_bad_input() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;

        // text フィールドが無い → deserialize 失敗 → AppError::Validation
        let err = uc
            .run_json(json!({ "wrong": "field" }), &ctx)
            .expect_err("should reject invalid input");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
```

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod audit;
pub mod driver;
pub mod risk;
pub mod traits;

pub use audit::{AuditEntry, AuditSink, InMemoryAuditSink, NoOpAuditSink};
pub use driver::Driver;
pub use risk::Risk;
pub use traits::{ErasedUseCase, UseCase};
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/traits.rs` の先頭（tests の前）に追加:

```rust
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::Risk;

/// 実装者が書く型安全な use case。関連型で Input/Output を型付けする。
/// `run` は同期（async 対応は Phase 4-5）。
pub trait UseCase {
    type Input: DeserializeOwned;
    type Output: Serialize;

    fn name(&self) -> &'static str;

    /// 実効 Risk。input を参照できる（archive のプラン依存 Risk 等）。
    /// 多くの use case は input を無視して固定 Risk を返す。
    fn risk(&self, input: &Self::Input) -> Risk;

    fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError>;
}

/// dyn 化のための消去層。`serde_json::Value` 境界で叩ける。
/// 実装は下のブランケットで `UseCase` から自動導出される（手書き不要）。
pub trait ErasedUseCase {
    fn name(&self) -> &str;
    fn risk_json(&self, input: &Value) -> Result<Risk, AppError>;
    fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError>;
}

impl<T: UseCase> ErasedUseCase for T {
    fn name(&self) -> &str {
        UseCase::name(self)
    }

    fn risk_json(&self, input: &Value) -> Result<Risk, AppError> {
        let typed: T::Input = serde_json::from_value(input.clone()).map_err(|e| {
            AppError::Validation(format!("invalid input for {}: {e}", UseCase::name(self)))
        })?;
        Ok(self.risk(&typed))
    }

    fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError> {
        let typed: T::Input = serde_json::from_value(input).map_err(|e| {
            AppError::Validation(format!("invalid input for {}: {e}", UseCase::name(self)))
        })?;
        let output = self.run(typed, ctx)?;
        serde_json::to_value(output)
            .map_err(|e| AppError::Validation(format!("failed to serialize output: {e}")))
    }
}
```

- [ ] **Step 3: テストが失敗 → 実装 → 成功を確認**

Run: `cd src-tauri && cargo test --lib usecase::traits -- --test-threads=4`
Expected: 4 テスト（roundtrip / name / risk_json / bad_input）が PASS。

- [ ] **Step 4: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/traits.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): UseCase traitとErasedUseCaseブランケット実装

型安全なジェネリクスtraitをValue境界に自動消去。dyn化の要。"
```

---

## Task 5: Registry

**Files:**
- Create: `src-tauri/src/usecase/registry.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Test: `src-tauri/src/usecase/registry.rs`（`#[cfg(test)]`）

**Interfaces:**
- Consumes: `ErasedUseCase`（Task 4）、`UseCase`（Task 4）
- Produces:
  - `pub struct Registry`
  - `Registry::new() -> Registry`
  - `Registry::register<T: UseCase + 'static>(&mut self, uc: T)`
  - `Registry::lookup(&self, name: &str) -> Option<&dyn ErasedUseCase>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/registry.rs` を新規作成。Task 4 のテストと同じ Echo パターンのダミー UseCase を使う（Task をまたいでコードを再掲する）:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::context::Ctx;
    use crate::error::AppError;
    use crate::usecase::{Risk, UseCase};

    #[derive(Deserialize)]
    struct EchoInput {
        text: String,
    }

    #[derive(Serialize)]
    struct EchoOutput {
        echoed: String,
    }

    struct EchoUseCase;

    impl UseCase for EchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;

        fn name(&self) -> &'static str {
            "echo"
        }

        fn risk(&self, _input: &Self::Input) -> Risk {
            Risk::Read
        }

        fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(EchoOutput {
                echoed: input.text,
            })
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let mut reg = Registry::new();
        reg.register(EchoUseCase);

        let uc = reg.lookup("echo").expect("echo should be registered");
        assert_eq!(uc.name(), "echo");
    }

    #[test]
    fn test_lookup_unknown_returns_none() {
        let reg = Registry::new();
        assert!(reg.lookup("missing").is_none());
    }
}
```

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod audit;
pub mod driver;
pub mod registry;
pub mod risk;
pub mod traits;

pub use audit::{AuditEntry, AuditSink, InMemoryAuditSink, NoOpAuditSink};
pub use driver::Driver;
pub use registry::Registry;
pub use risk::Risk;
pub use traits::{ErasedUseCase, UseCase};
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/registry.rs` の先頭（tests の前）に追加:

```rust
use std::collections::HashMap;

use crate::usecase::{ErasedUseCase, UseCase};

/// name → UseCase のマップ。3 driver がここを引いて同じ能力セットを共有する。
/// MCP の tool 一覧・JSON Schema 自動導出は将来このレジストリに乗る（Phase 5-1）。
pub struct Registry {
    map: HashMap<&'static str, Box<dyn ErasedUseCase>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// 型安全な UseCase を登録する。ブランケット実装により Box<dyn ErasedUseCase> に消去される。
    pub fn register<T: UseCase + 'static>(&mut self, uc: T) {
        let name = UseCase::name(&uc);
        self.map.insert(name, Box::new(uc));
    }

    /// name で消去済み UseCase を引く。
    pub fn lookup(&self, name: &str) -> Option<&dyn ErasedUseCase> {
        self.map.get(name).map(|b| b.as_ref())
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::registry -- --test-threads=4`
Expected: `test_register_and_lookup` と `test_lookup_unknown_returns_none` が PASS。

- [ ] **Step 4: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/registry.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): name→UseCaseのRegistryを追加

3 driverが共有する能力セットの単一の置き場所。"
```

---

## Task 6: gate

**Files:**
- Create: `src-tauri/src/usecase/gate.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Test: `src-tauri/src/usecase/gate.rs`（`#[cfg(test)]`）

**Interfaces:**
- Consumes: `Risk`（Task 1）、`Driver`（Task 2）、`AppError`（error.rs）
- Produces:
  - `pub fn check(risk: Risk, driver: Driver) -> Result<(), AppError>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/gate.rs` を新規作成:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::usecase::{Driver, Risk};

    #[test]
    fn test_read_passes_for_all_drivers() {
        for driver in [Driver::Ui, Driver::Mcp, Driver::Agent] {
            assert!(check(Risk::Read, driver).is_ok(), "Read は {driver:?} で通過する");
        }
    }

    #[test]
    fn test_reversible_is_rejected() {
        let err = check(Risk::Reversible, Driver::Ui).expect_err("Reversible は 4-2 では拒否");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_sensitive_is_rejected() {
        let err = check(Risk::Sensitive, Driver::Ui).expect_err("Sensitive は 4-2 では拒否");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
```

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod audit;
pub mod driver;
pub mod gate;
pub mod registry;
pub mod risk;
pub mod traits;

pub use audit::{AuditEntry, AuditSink, InMemoryAuditSink, NoOpAuditSink};
pub use driver::Driver;
pub use registry::Registry;
pub use risk::Risk;
pub use traits::{ErasedUseCase, UseCase};
```

（注: `gate` は関数モジュールなので `pub use` での型再エクスポートはしない。`usecase::gate::check` で呼ぶ。）

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/gate.rs` の先頭（tests の前）に追加:

```rust
use crate::error::AppError;
use crate::usecase::{Driver, Risk};

/// Risk ゲート。実行してよいか（誰が）の認可判定。
/// 4-2 では Read のみ通過。Reversible/Sensitive のゲート本体（承認キュー投入・
/// driver 分岐）は Phase 4-4。read 系しか載らないため実害はない。
pub fn check(risk: Risk, _driver: Driver) -> Result<(), AppError> {
    match risk {
        Risk::Read => Ok(()),
        Risk::Reversible | Risk::Sensitive => Err(AppError::Validation(format!(
            "risk gate not yet implemented for {risk:?} (Phase 4-4)"
        ))),
    }
}
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::gate -- --test-threads=4`
Expected: 3 テスト（read_passes / reversible_rejected / sensitive_rejected）が PASS。

- [ ] **Step 4: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/gate.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): Riskゲートの骨格を追加

Read=通過、Reversible/Sensitive=拒否（4-4で承認キューに置換）。
driver引数はシグネチャに含め拡張点を固定。"
```

---

## Task 7: Ctx に driver / audit アクセサを追加

**Files:**
- Modify: `src-tauri/src/context.rs`
- Test: `src-tauri/src/context.rs`（既存 `#[cfg(test)]` に追加）

**Interfaces:**
- Consumes: `Driver`（Task 2）、`AuditSink` / `NoOpAuditSink`（Task 3）
- Produces:
  - `Ctx::driver(&self) -> Driver`
  - `Ctx::audit(&self) -> &dyn AuditSink`

**この Task のポイント:** 既存 `Ctx::new` / `new_for_test` の引数は変えない（4-1 で確立した 2 command への波及を避ける）。driver は `Ctx::new`（commands 経由）では `Driver::Ui` 固定、`new_for_test` でも `Ui` 既定。audit は `&'static NoOpAuditSink`（`const`）を返す既定実装にする。SQLite シンク差し替えは 4-4 で Ctx にフィールドを足して行う。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/context.rs` の `#[cfg(test)] mod tests` の末尾（`test_sync_locks_accessor_returns_shared_state` の後）に追加:

```rust
    #[test]
    fn test_driver_defaults_to_ui() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        assert_eq!(ctx.driver(), crate::usecase::Driver::Ui);
    }

    #[test]
    fn test_audit_sink_is_available() {
        use crate::usecase::{AuditEntry, Risk};
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        // NoOp なので record しても panic しない（返り値が &dyn AuditSink であることの確認）
        ctx.audit()
            .record(AuditEntry::new("x", Risk::Read, ctx.driver()));
    }
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/context.rs` の import に追加:

```rust
use crate::usecase::{AuditSink, Driver, NoOpAuditSink};
```

`Ctx<'a>` 構造体に `driver` フィールドを追加:

```rust
pub struct Ctx<'a> {
    db: &'a DbState,
    secure_store: Option<&'a SecureStore>,
    pending: &'a PendingClassifications,
    batches: &'a ClassifyBatches,
    sync_locks: &'a SyncLocks,
    driver: Driver,
}
```

`new` の構築で `driver: Driver::Ui` を設定:

```rust
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
            driver: Driver::Ui,
        }
    }
```

`new_for_test` にも `driver: Driver::Ui` を追加:

```rust
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
            driver: Driver::Ui,
        }
    }
```

`sync_locks()` アクセサの後に driver / audit アクセサを追加:

```rust
    /// この Ctx を構築した driver（ゲートの判定材料）。
    pub fn driver(&self) -> Driver {
        self.driver
    }

    /// 監査シンク。4-2 は NoOp 固定（read 系は監査対象外）。
    /// SQLite シンクへの差し替えは Phase 4-4。
    pub fn audit(&self) -> &dyn AuditSink {
        const SINK: &NoOpAuditSink = &NoOpAuditSink;
        SINK
    }
```

`context.rs` の doc コメント（10 行目付近「Risk ゲート等は Phase 4-4 で載せる」）を修正:

```rust
/// 全 driver（commands / 将来の MCP・agent）が共有する借用コンテキスト。
/// Tauri が所有する各 managed State への参照を束ね、driver 情報と監査シンクを持つ。
/// Risk ゲートの骨格は Phase 4-2、ゲート本体（承認キュー）と監査永続化は Phase 4-4。
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib context:: -- --test-threads=4`
Expected: 既存 2 テスト + 新規 2 テスト（driver_defaults_to_ui / audit_sink_is_available）が PASS。

- [ ] **Step 4: クレート全体のビルドと既存テスト**

Run: `cd src-tauri && cargo build && cargo test --lib -- --test-threads=4`
Expected: ビルド成功、全 PASS（既存 command の Ctx 構築は引数不変なので影響なし）。

- [ ] **Step 5: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/context.rs
git commit -m "feat(context): Ctxにdriver/audit アクセサを追加

driverはUi固定（commands経由）、auditはNoOp固定。gateとdispatchが
受け取る拡張点。フェーズコメントをRiskゲート=4-2に整合。"
```

---

## Task 8: dispatch

**Files:**
- Create: `src-tauri/src/usecase/dispatch.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Test: `src-tauri/src/usecase/dispatch.rs`（`#[cfg(test)]`）

**Interfaces:**
- Consumes: `Registry`（Task 5）、`gate::check`（Task 6）、`Risk`（Task 1）、`Ctx`（Task 7）、`AuditEntry`（Task 3）、`AppError`
- Produces:
  - `pub fn dispatch(registry: &Registry, name: &str, input: Value, ctx: &Ctx) -> Result<Value, AppError>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/dispatch.rs` を新規作成。read 系 Echo（通過）と、ダミー Reversible（ゲート拒否）の 2 UseCase を登録して検証する:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::error::AppError;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::{Registry, Risk, UseCase};

    #[derive(Deserialize)]
    struct EchoInput {
        text: String,
    }
    #[derive(Serialize)]
    struct EchoOutput {
        echoed: String,
    }
    struct EchoUseCase;
    impl UseCase for EchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;
        fn name(&self) -> &'static str {
            "echo"
        }
        fn risk(&self, _input: &Self::Input) -> Risk {
            Risk::Read
        }
        fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(EchoOutput { echoed: input.text })
        }
    }

    // ゲートに弾かれることを見るためのダミー Reversible use case
    #[derive(Deserialize)]
    struct NoInput {}
    #[derive(Serialize)]
    struct NoOutput {}
    struct DangerUseCase;
    impl UseCase for DangerUseCase {
        type Input = NoInput;
        type Output = NoOutput;
        fn name(&self) -> &'static str {
            "danger"
        }
        fn risk(&self, _input: &Self::Input) -> Risk {
            Risk::Reversible
        }
        fn run(&self, _input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(NoOutput {})
        }
    }

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    fn build_registry() -> Registry {
        let mut reg = Registry::new();
        reg.register(EchoUseCase);
        reg.register(DangerUseCase);
        reg
    }

    #[test]
    fn test_dispatch_read_usecase_succeeds() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry();

        let out = dispatch(&reg, "echo", json!({ "text": "hi" }), &ctx)
            .expect("read use case should dispatch");
        assert_eq!(out, json!({ "echoed": "hi" }));
    }

    #[test]
    fn test_dispatch_unknown_name_errors() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry();

        let err = dispatch(&reg, "nope", json!({}), &ctx)
            .expect_err("unknown name should error");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_dispatch_reversible_is_gated() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry();

        let err = dispatch(&reg, "danger", json!({}), &ctx)
            .expect_err("Reversible should be gated in 4-2");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
```

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod audit;
pub mod dispatch;
pub mod driver;
pub mod gate;
pub mod registry;
pub mod risk;
pub mod traits;

pub use audit::{AuditEntry, AuditSink, InMemoryAuditSink, NoOpAuditSink};
pub use dispatch::dispatch;
pub use driver::Driver;
pub use registry::Registry;
pub use risk::Risk;
pub use traits::{ErasedUseCase, UseCase};
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/dispatch.rs` の先頭（tests の前）に追加:

```rust
use serde_json::Value;

use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::{gate, AuditEntry, Registry, Risk};

/// 単一の chokepoint。3 driver すべてがこの 1 関数を通る（特権的な裏口なし）。
/// lookup → risk 判定 → ゲート → 監査 → 実行 のパイプライン。
pub fn dispatch(
    registry: &Registry,
    name: &str,
    input: Value,
    ctx: &Ctx,
) -> Result<Value, AppError> {
    let uc = registry
        .lookup(name)
        .ok_or_else(|| AppError::Validation(format!("unknown use case: {name}")))?;

    let risk = uc.risk_json(&input)?;
    gate::check(risk, ctx.driver())?;

    // 4-2 では Read のみ載るため実質未発火。記録の実体（SQLite）は 4-4。
    if risk != Risk::Read {
        ctx.audit()
            .record(AuditEntry::new(name, risk, ctx.driver()));
    }

    uc.run_json(input, ctx)
}
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::dispatch -- --test-threads=4`
Expected: 3 テスト（read_succeeds / unknown_errors / reversible_gated）が PASS。

- [ ] **Step 4: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/dispatch.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): dispatchバス（単一chokepoint）を追加

lookup→risk→gate→audit呼び出し点→run。3 driver共有の入口。
Read通過・Reversible/Sensitiveはゲート拒否。"
```

---

## Task 9: SearchMailsUseCase（read 系の実例をレジストリに載せる）

**Files:**
- Create: `src-tauri/src/usecase/cases/mod.rs`
- Create: `src-tauri/src/usecase/cases/search.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Test: `src-tauri/src/usecase/cases/search.rs`（`#[cfg(test)]`）

**Interfaces:**
- Consumes: `UseCase`（Task 4）、`Risk`（Task 1）、`Ctx`（Task 7）、`dispatch`（Task 8）、`Registry`（Task 5）、`db::search::search_mails`（既存）、`SearchResult`（既存 models/mail.rs）
- Produces:
  - `pub struct SearchMailsUseCase;`（`UseCase` 実装、name = "search_mails"、Risk::Read）
  - `pub struct SearchMailsInput { pub account_id: String, pub query: String }`
  - `pub fn register_read_cases(registry: &mut Registry)` — read 系をまとめて登録するヘルパ

**注（実コード確認済み）:** `db::search::search_mails(conn: &Connection, account_id: &str, query: &str, limit: u32) -> Result<Vec<SearchResult>, AppError>`。`SearchResult`（models/mail.rs:41）は `Serialize + Deserialize` 導出済み。limit は既存 command と同じ 100。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/cases/search.rs` を新規作成:

```rust
// 本体は Step 2 で埋める

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::{insert_test_mail, setup_db};
    use crate::usecase::{dispatch, Registry, Risk, UseCase};

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[test]
    fn test_search_usecase_declares_read_risk() {
        let uc = SearchMailsUseCase;
        let input = SearchMailsInput {
            account_id: "acc1".into(),
            query: "hello".into(),
        };
        assert_eq!(uc.risk(&input), Risk::Read);
        assert_eq!(uc.name(), "search_mails");
    }

    #[test]
    fn test_search_via_dispatch_matches_direct_query() {
        let (db, pending, batches, locks) = build_states();
        // setup_db 済み。件名に "Report" を含むメールを1件入れる
        {
            let conn = db.0.lock().unwrap();
            insert_test_mail(&conn, "m1", "Quarterly Report");
        }
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let mut reg = Registry::new();
        register_read_cases(&mut reg);

        let out = dispatch(
            &reg,
            "search_mails",
            json!({ "account_id": "acc1", "query": "Report" }),
            &ctx,
        )
        .expect("search should dispatch");

        // 出力は Vec<SearchResult> の JSON。1件ヒットする
        let arr = out.as_array().expect("output is a JSON array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["mail"]["id"], "m1");
    }

    #[test]
    fn test_search_via_dispatch_empty_query_returns_empty() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let mut reg = Registry::new();
        register_read_cases(&mut reg);

        let out = dispatch(
            &reg,
            "search_mails",
            json!({ "account_id": "acc1", "query": "" }),
            &ctx,
        )
        .expect("empty query should dispatch");
        assert_eq!(out, json!([]));
    }
}
```

`src-tauri/src/usecase/cases/mod.rs` を新規作成:

```rust
pub mod search;

pub use search::{register_read_cases, SearchMailsInput, SearchMailsUseCase};
```

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod audit;
pub mod cases;
pub mod dispatch;
pub mod driver;
pub mod gate;
pub mod registry;
pub mod risk;
pub mod traits;

pub use audit::{AuditEntry, AuditSink, InMemoryAuditSink, NoOpAuditSink};
pub use dispatch::dispatch;
pub use driver::Driver;
pub use registry::Registry;
pub use risk::Risk;
pub use traits::{ErasedUseCase, UseCase};
```

- [ ] **Step 2: 本体を実装**

`src-tauri/src/usecase/cases/search.rs` の先頭（tests の前）に追加:

```rust
use serde::Deserialize;

use crate::context::Ctx;
use crate::db::search;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::usecase::{Registry, Risk, UseCase};

/// `search_mails` UseCase の入力。
#[derive(Deserialize)]
pub struct SearchMailsInput {
    pub account_id: String,
    pub query: String,
}

/// 全文検索の read 系 UseCase（バスに載せる最初の実例）。
pub struct SearchMailsUseCase;

impl UseCase for SearchMailsUseCase {
    type Input = SearchMailsInput;
    type Output = Vec<SearchResult>;

    fn name(&self) -> &'static str {
        "search_mails"
    }

    fn risk(&self, _input: &Self::Input) -> Risk {
        Risk::Read
    }

    fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| search::search_mails(conn, &input.account_id, &input.query, 100))
    }
}

/// read 系 UseCase をレジストリにまとめて登録する。
/// 水平展開（他の read 系コマンドの UseCase 化）はここに足していく。
pub fn register_read_cases(registry: &mut Registry) {
    registry.register(SearchMailsUseCase);
}
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::cases -- --test-threads=4`
Expected: 3 テスト（declares_read_risk / dispatch_matches / empty_query）が PASS。

- [ ] **Step 4: クレート全体で最終確認**

Run: `cd src-tauri && cargo build && cargo test --lib -- --test-threads=4`
Expected: ビルド成功、全 PASS。

- [ ] **Step 5: 整形してコミット**（ユーザー指示後）

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/cases/mod.rs src-tauri/src/usecase/cases/search.rs src-tauri/src/usecase/mod.rs
git commit -m "feat(usecase): SearchMailsUseCaseをバスに載せる

read系の実例。dispatch経由で既存db::searchと同結果。
register_read_casesで水平展開の受け皿を用意。"
```

---

## 完了条件（Phase 4-2 の Definition of Done）

- `usecase` モジュールに `UseCase` / `ErasedUseCase`（ブランケット）/ `Registry` / `Risk` / `Driver` / `gate::check` / `AuditSink`（NoOp/InMemory）/ `dispatch` が存在する。
- `SearchMailsUseCase` がレジストリ登録され、`dispatch("search_mails", …)` で既存 search と同結果を返す。
- `gate::check` が Read=通過、Reversible/Sensitive=拒否。
- Ctx に `driver()` / `audit()` が生え、既存 command は `Driver::Ui` で構築（`Ctx::new` の引数は不変）。
- `cargo build` と `cargo test --lib` が緑。既存挙動不変。
- `context.rs` のフェーズコメントが Risk ゲート=4-2 に整合（引き継ぎ課題 2）。

## Self-Review 結果

- **Spec coverage:** 設計書の各コンポーネント（§1 型付け=Task 4、§2 レジストリ=Task 5、§3 Risk=Task 1、§4 Driver+Ctx=Task 2,7、§5 dispatch=Task 8、§6 gate=Task 6、§7 AuditSink=Task 3、§8 SearchMailsUseCase=Task 9、§9 フェーズコメント整合=Task 7）を全てタスク化。スコープ外（4-3/4-4/4-5/Phase5）はタスク化しない。✅
- **Placeholder scan:** 各ステップに実コードと実コマンドを記載。「TBD」「後で」なし。ダミー UseCase（Echo）は Task 4/5/8 で再掲（out-of-order 読解のため）。✅
- **Type consistency:** `UseCase::name -> &'static str` と `ErasedUseCase::name -> &str` の使い分け、`risk_json(&Value)` / `run_json(Value)` の引数型、`dispatch(&Registry, &str, Value, &Ctx) -> Result<Value, AppError>`、`Ctx::driver() -> Driver` / `audit() -> &dyn AuditSink`、`register_read_cases(&mut Registry)` が Task 間で一貫。`search_mails(conn, &str, &str, 100)` の第4引数 limit=100 は既存 command と一致。✅
- **audit 注入の未確定事項:** `Ctx::audit()` が `const SINK: &NoOpAuditSink` を返す方式で確定（Task 7）。既存 `Ctx::new` の引数不変を担保。✅
```
