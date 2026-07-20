# CLI / MCP driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pigeon の GUI でできる主要操作を、CLI（`pigeon-cli`）と MCP サーバー（`pigeon-cli mcp`）からも実行できるようにする。

**Architecture:** 既存の `usecase/dispatch.rs` が全 driver の単一 chokepoint である構造をそのまま使う。`Driver` enum に `Cli` を追加し、stdin の TTY 有無で Ui 相当 / Mcp 相当の Risk ポリシーを切り分ける。CLI と MCP は引数を `serde_json::Value` にして `dispatch` に渡すだけの薄い層に留め、`dispatch` から下は変更しない。

**Tech Stack:** Rust 2021 / Tauri 2 / rusqlite 0.31 / async-trait / serde_json / tokio。新規依存として `clap`（CLI 引数）と `schemars`（JSON Schema 導出）を追加する。

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない（agent.md）
- アプリケーションエラーは `thiserror` で定義した `AppError` を使う
- TDD: Red → Green → Refactor。テストを先に書く
- コミットは Conventional Commits 形式。scope は `cli`, `mcp`, `usecase`, `ctx` 等
- 1 コミット = 1 意図。PR は Single Concern
- `dispatch` 関数本体は変更しない（特権的な裏口を作らない / ADR 0004）
- 既存の Tauri command の**関数名は変更しない**（`invoke_handler` の登録リストと frontend の呼び出しを壊さないため）
- 進捗送出・監査記録の失敗は本処理を止めない（ベストエフォート）
- 設計書: `docs/design/2026-07-20-cli-mcp-driver-design.md`

## File Structure

**新規作成**

| ファイル | 責務 |
|---|---|
| `src-tauri/src/usecase/progress.rs` | `ProgressSink` trait と `NoOpProgressSink` |
| `src-tauri/src/bin/pigeon-cli.rs` | CLI エントリポイント。引数パース → dispatch → 出力 |
| `src-tauri/src/cli/mod.rs` | CLI モジュールルート |
| `src-tauri/src/cli/tty.rs` | TTY 判定（注入可能） |
| `src-tauri/src/cli/runtime.rs` | DB/SecureStore を開き `Ctx` を組み立てる。排他制御 |
| `src-tauri/src/cli/output.rs` | 人間向けテキスト / `--json` の出力整形 |
| `src-tauri/src/cli/progress.rs` | stderr へ進捗を出す `ProgressSink` 実装 |
| `src-tauri/src/mcp/mod.rs` | MCP モジュールルート |
| `src-tauri/src/mcp/protocol.rs` | JSON-RPC 2.0 の型定義 |
| `src-tauri/src/mcp/server.rs` | stdio ループ、initialize / tools/list / tools/call |

**変更**

| ファイル | 変更内容 |
|---|---|
| `src-tauri/src/usecase/driver.rs` | `Driver::Cli` 追加 |
| `src-tauri/src/usecase/gate.rs` | `Cli` の分岐追加 |
| `src-tauri/src/usecase/registry.rs` | 列挙 API 追加 |
| `src-tauri/src/usecase/traits.rs` | schema 導出フック追加 |
| `src-tauri/src/usecase/mod.rs` | re-export 追加 |
| `src-tauri/src/context.rs` | `cfg(test)` 解除、`progress` フィールド追加 |
| `src-tauri/src/usecase/cases/mailbox.rs` | read 系 UseCase 追加 |
| `src-tauri/src/usecase/cases/project.rs` | `get_projects` UseCase 追加 |
| `src-tauri/src/usecase/cases/sync.rs` | 新規。`sync_account` UseCase |
| `src-tauri/src/usecase/cases/classify.rs` | 新規。`classify_batch` UseCase |
| `src-tauri/src/usecase/cases/mod.rs` | 新モジュール登録 |
| `src-tauri/src/commands/*.rs` | 移行した command を dispatch 経由に |
| `src-tauri/src/lib.rs` | `pub mod cli; pub mod mcp;`、GUI 側 ProgressSink |
| `src-tauri/Cargo.toml` | `[[bin]]`、`clap`、`schemars` |

## タスク依存関係

```
Task 1 (Driver::Cli + gate)
Task 2 (Ctx cfg解除)
Task 3 (ProgressSink)          ← Task 2
Task 4 (Registry列挙 + schema)
Task 5 (read系4つ載せ替え)     ← Task 4
Task 6 (sync載せ替え)          ← Task 3
Task 7 (classify載せ替え)      ← Task 3
Task 8 (TTY判定)               ← Task 1
Task 9 (CLI runtime + 排他)    ← Task 2, 8
Task 10 (CLI call + 出力)      ← Task 4, 9
Task 11 (CLIサブコマンド)      ← Task 10
Task 12 (MCPサーバー)          ← Task 4, 9
```

Task 1〜4 は互いに独立。5/6/7 も互いに独立。

---

### Task 1: `Driver::Cli` と gate マトリクスの拡張

**Files:**
- Modify: `src-tauri/src/usecase/driver.rs`
- Modify: `src-tauri/src/usecase/gate.rs`

**Interfaces:**
- Consumes: 既存の `Risk`, `Driver`, `GateOutcome`
- Produces: `Driver::CliInteractive`, `Driver::CliAutomated`。`gate::check(risk, driver) -> GateOutcome` のシグネチャは不変

**設計判断:** `Driver::Cli` を 1 つ足して TTY 有無を別に持たせると、`gate::check` が 2 引数では判定できなくなる。`Driver` を `CliInteractive` / `CliAutomated` の 2 値に分けることで `check` のシグネチャを変えずに済み、gate は純関数のままテストしやすい。TTY 判定は Ctx 構築時（Task 8, 9）に 1 回だけ行い、以降は driver 値が事実を運ぶ。

- [ ] **Step 1: driver.rs に失敗するテストを書く**

`src-tauri/src/usecase/driver.rs` の `#[cfg(test)] mod tests` に追加（モジュールが無ければ作る）:

```rust
#[test]
fn test_cli_drivers_as_str() {
    assert_eq!(Driver::CliInteractive.as_str(), "cli_interactive");
    assert_eq!(Driver::CliAutomated.as_str(), "cli_automated");
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib usecase::driver 2>&1 | tail -20`
Expected: コンパイルエラー `no variant named CliInteractive found for enum Driver`

- [ ] **Step 3: Driver に 2 バリアント追加**

`src-tauri/src/usecase/driver.rs` の enum と `as_str` に追加:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Ui,
    /// 対話端末から起動された CLI。人間の明示操作とみなす
    CliInteractive,
    /// 非対話（パイプ・エージェント経由）で起動された CLI
    CliAutomated,
    Mcp,
    Agent,
}

impl Driver {
    pub fn as_str(self) -> &'static str {
        match self {
            Driver::Ui => "ui",
            Driver::CliInteractive => "cli_interactive",
            Driver::CliAutomated => "cli_automated",
            Driver::Mcp => "mcp",
            Driver::Agent => "agent",
        }
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::driver 2>&1 | tail -20`
Expected: PASS。ただし `gate::check` の match が非網羅になりコンパイルエラーが出る場合は Step 5 と同時に直す

- [ ] **Step 5: gate に失敗するテストを書く**

`src-tauri/src/usecase/gate.rs` の tests に追加:

```rust
#[test]
fn test_read_and_reversible_pass_for_cli_drivers() {
    for driver in [Driver::CliInteractive, Driver::CliAutomated] {
        assert_eq!(check(Risk::Read, driver), GateOutcome::Allow);
        assert_eq!(check(Risk::Reversible, driver), GateOutcome::Allow);
    }
}

#[test]
fn test_sensitive_from_interactive_cli_is_allowed() {
    // 対話端末での実行は人間の明示操作そのもの（Ui と同じ扱い）
    assert_eq!(check(Risk::Sensitive, Driver::CliInteractive), GateOutcome::Allow);
}

#[test]
fn test_sensitive_from_automated_cli_requires_approval() {
    // 非対話 = エージェント経由の可能性があるため承認キューへ
    assert_eq!(
        check(Risk::Sensitive, Driver::CliAutomated),
        GateOutcome::RequireApproval
    );
}
```

- [ ] **Step 6: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib usecase::gate 2>&1 | tail -20`
Expected: FAIL（match 非網羅のコンパイルエラー、または assert 失敗）

- [ ] **Step 7: gate::check を実装**

`src-tauri/src/usecase/gate.rs`:

```rust
pub fn check(risk: Risk, driver: Driver) -> GateOutcome {
    match (risk, driver) {
        (Risk::Read | Risk::Reversible, _) => GateOutcome::Allow,
        // UI と対話 CLI の Sensitive は人間の明示操作そのものが承認
        (Risk::Sensitive, Driver::Ui | Driver::CliInteractive) => GateOutcome::Allow,
        // LLM 起点・非対話起点の Sensitive は人間の承認まで保留（Phase 5-2 の承認 UI で消費）
        (Risk::Sensitive, Driver::CliAutomated | Driver::Mcp | Driver::Agent) => {
            GateOutcome::RequireApproval
        }
    }
}
```

- [ ] **Step 8: 全テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase 2>&1 | tail -20`
Expected: PASS（既存の driver/gate/dispatch テストも含めて）

- [ ] **Step 9: コミット**

```bash
cd src-tauri && cargo fmt -- src/usecase/driver.rs src/usecase/gate.rs
git add src-tauri/src/usecase/driver.rs src-tauri/src/usecase/gate.rs
git commit -m "feat(usecase): CLI driverをRiskゲートに追加

対話端末からの起動(CliInteractive)はUi相当、非対話
(CliAutomated)はMcp相当のポリシーとする。TTY判定を
Ctx構築時に済ませ、gateは純関数のまま保つ。"
```

---

### Task 2: `Ctx` の非 UI driver 構築を本番で可能にする

**Files:**
- Modify: `src-tauri/src/context.rs`

**Interfaces:**
- Consumes: `Driver`（Task 1 は不要。既存の `Driver::Mcp` でテストできる）
- Produces: `Ctx::with_driver(self, driver: Driver) -> Self`（`cfg(test)` なし）、`Ctx::new_headless(...)`

**設計判断:** `Ctx::new` は `SecureStoreState`（Tauri State のラッパ）を要求するが、CLI は Tauri State を持たない。`SecureStore` を直接受ける `new_headless` を追加する。既存の `with_secure_store` は `cfg(test)` 付きで同じ役割なので、これを本番用に昇格させる形でもよいが、CLI では secure_store は必須（IMAP 認証に要る）なので、Option にせず必須引数として受け取るコンストラクタを分ける方が誤用しにくい。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/context.rs` の tests に追加:

```rust
#[test]
fn test_new_headless_sets_driver_and_secure_store() {
    let (db, pending, batches, sync_locks) = build_states();
    let store = SecureStore::new_in_memory();
    let ctx = Ctx::new_headless(
        &db,
        &store,
        &pending,
        &batches,
        &sync_locks,
        Driver::CliAutomated,
    );
    assert_eq!(ctx.driver(), Driver::CliAutomated);
    assert!(ctx.secure_store().is_ok());
}
```

`SecureStore::new_in_memory()` の正確な構築方法は `src-tauri/src/secure_store.rs` を確認すること。既存テストが `InMemory` バリアントを使っている箇所（`grep -rn "InMemory" src/`）に合わせる。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib context:: 2>&1 | tail -20`
Expected: FAIL `no function or associated item named new_headless`

- [ ] **Step 3: `new_headless` を実装し `with_driver` の cfg を外す**

`src-tauri/src/context.rs`:

```rust
    /// GUI を伴わない driver（CLI / MCP）用のコンストラクタ。
    /// Tauri State ではなく SecureStore を直接受け取る。
    pub fn new_headless(
        db: &'a DbState,
        secure_store: &'a SecureStore,
        pending: &'a PendingClassifications,
        batches: &'a ClassifyBatches,
        sync_locks: &'a SyncLocks,
        driver: Driver,
    ) -> Self {
        Self {
            db,
            secure_store: Some(secure_store),
            approved_attachments: None,
            pending,
            batches,
            sync_locks,
            driver,
            audit: None,
            progress: None,
        }
    }
```

注: `progress` フィールドは Task 3 で追加する。Task 3 を先に実施する場合はこの行を含め、後にする場合はこの行を除く。**Task 2 と Task 3 を連続で実施することを推奨する。**

`with_driver` から `#[cfg(test)]` を削除:

```rust
    /// driver を差し替える。非 UI driver（CLI / MCP）の構築とテストで使う。
    pub fn with_driver(mut self, driver: Driver) -> Self {
        self.driver = driver;
        self
    }
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib context:: 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
cd src-tauri && cargo fmt -- src/context.rs
git add src-tauri/src/context.rs
git commit -m "feat(ctx): 非UI driverのCtxを本番コードから構築可能にする

with_driverのcfg(test)を外し、SecureStoreを直接受ける
new_headlessを追加。CLI/MCP driverの接続点。"
```

---

### Task 3: `ProgressSink` の導入

**Files:**
- Create: `src-tauri/src/usecase/progress.rs`
- Modify: `src-tauri/src/usecase/mod.rs`
- Modify: `src-tauri/src/context.rs`

**Interfaces:**
- Consumes: なし
- Produces:
  - `trait ProgressSink: Send + Sync { fn emit(&self, event: &str, payload: &serde_json::Value); }`
  - `struct NoOpProgressSink;`
  - `Ctx::with_progress(self, sink: &'a dyn ProgressSink) -> Self`
  - `Ctx::progress(&self) -> &dyn ProgressSink`

**設計判断:** イベント名を `&str`、ペイロードを `Value` にして、既存の Tauri イベント名（`sync-progress` / `classify-progress`）と payload 構造をそのまま通せる形にする。UseCase 側が driver ごとの差を知らずに済む。`AuditSink` と同じ「`Option<&dyn Trait>` + 既定値フォールバック」の形に揃える。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/progress.rs` を新規作成:

```rust
use serde_json::Value;

/// 長時間処理の進捗通知。driver ごとに出力先が異なる（GUI: emit / CLI: stderr / MCP: 破棄）。
/// 送出失敗で本処理を止めない（ベストエフォート）。
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: &str, payload: &Value);
}

/// 進捗を捨てる既定実装。
pub struct NoOpProgressSink;

impl ProgressSink for NoOpProgressSink {
    fn emit(&self, _event: &str, _payload: &Value) {}
}

#[cfg(test)]
pub struct RecordingProgressSink {
    pub events: std::sync::Mutex<Vec<(String, Value)>>,
}

#[cfg(test)]
impl RecordingProgressSink {
    pub fn new() -> Self {
        Self { events: std::sync::Mutex::new(Vec::new()) }
    }
}

#[cfg(test)]
impl ProgressSink for RecordingProgressSink {
    fn emit(&self, event: &str, payload: &Value) {
        if let Ok(mut v) = self.events.lock() {
            v.push((event.to_string(), payload.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_sink_does_not_panic() {
        NoOpProgressSink.emit("sync-progress", &serde_json::json!({"done": 1}));
    }

    #[test]
    fn test_recording_sink_captures_events() {
        let sink = RecordingProgressSink::new();
        sink.emit("sync-progress", &serde_json::json!({"done": 3, "total": 10}));
        let events = sink.events.lock().expect("lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "sync-progress");
        assert_eq!(events[0].1["done"], 3);
    }
}
```

- [ ] **Step 2: mod.rs に登録**

`src-tauri/src/usecase/mod.rs` に追加:

```rust
pub mod progress;
```

re-export に追加:

```rust
pub use progress::{NoOpProgressSink, ProgressSink};
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase::progress 2>&1 | tail -20`
Expected: PASS（2 テスト）

- [ ] **Step 4: Ctx に progress を組み込む失敗テストを書く**

`src-tauri/src/context.rs` の tests に追加:

```rust
#[test]
fn test_ctx_progress_defaults_to_noop_and_can_be_replaced() {
    use crate::usecase::progress::RecordingProgressSink;

    let (db, pending, batches, sync_locks) = build_states();
    let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
    // 既定は NoOp。呼んでも panic しない
    ctx.progress().emit("x", &serde_json::json!({}));

    let sink = RecordingProgressSink::new();
    let ctx = ctx.with_progress(&sink);
    ctx.progress().emit("sync-progress", &serde_json::json!({"done": 1}));
    assert_eq!(sink.events.lock().expect("lock").len(), 1);
}
```

- [ ] **Step 5: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib context:: 2>&1 | tail -20`
Expected: FAIL `no method named with_progress`

- [ ] **Step 6: Ctx に progress フィールドとアクセサを追加**

`src-tauri/src/context.rs`。import に追加:

```rust
use crate::usecase::{AuditSink, Driver, NoOpProgressSink, ProgressSink, SqliteAuditSink};
```

struct にフィールド追加:

```rust
    /// None なら NoOpProgressSink（progress() 参照）。driver ごとに差し替える。
    progress: Option<&'a dyn ProgressSink>,
```

**すべてのコンストラクタ**（`new`, `new_headless`, `new_for_test`）の初期化に `progress: None,` を追加する。

メソッド追加:

```rust
    pub fn with_progress(mut self, sink: &'a dyn ProgressSink) -> Self {
        self.progress = Some(sink);
        self
    }

    pub fn progress(&self) -> &dyn ProgressSink {
        const DEFAULT: &NoOpProgressSink = &NoOpProgressSink;
        self.progress.unwrap_or(DEFAULT)
    }
```

- [ ] **Step 7: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -20`
Expected: PASS（全 lib テスト）

- [ ] **Step 8: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/progress.rs src-tauri/src/usecase/mod.rs src-tauri/src/context.rs
git commit -m "feat(usecase): 進捗シンクをCtxに追加

sync/classifyがAppHandleに直接依存していてバスに載らない
問題を解消する。出力先はdriverごとに差し替える
(GUI: emit / CLI: stderr / MCP: 破棄)。AuditSinkと同じ形。"
```

---

### Task 4: `Registry` の列挙 API と JSON Schema 導出

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/usecase/traits.rs`
- Modify: `src-tauri/src/usecase/registry.rs`
- Modify: `src-tauri/src/usecase/cases/*.rs`（全 Input 型に derive 追加）

**Interfaces:**
- Consumes: 既存の `UseCase` / `ErasedUseCase` / `Registry`
- Produces:
  - `UseCase::Input: DeserializeOwned + Send + schemars::JsonSchema`
  - `ErasedUseCase::input_schema(&self) -> serde_json::Value`
  - `ErasedUseCase::risk_static(&self) -> Option<Risk>` は**作らない**（risk は input 依存のため）
  - `Registry::names(&self) -> Vec<&'static str>`（ソート済み）
  - `Registry::describe(&self) -> Vec<UseCaseInfo>`
  - `struct UseCaseInfo { pub name: &'static str, pub input_schema: serde_json::Value }`

**設計判断:** `risk` は `&Self::Input` を取るため、input なしに静的な Risk を返せない。MCP の tool description には Risk を載せたいが、今回は載せない（YAGNI。必要なら後で `UseCase` に `fn static_risk() -> Option<Risk>` を足す）。

`names()` は `HashMap` の反復順が非決定的なのでソートして返す。`--help` や tools/list の出力が毎回変わると diff が読めなくなる。

- [ ] **Step 1: schemars を追加**

`src-tauri/Cargo.toml` の `[dependencies]` に追加:

```toml
schemars = "0.8"
```

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: 成功（依存が解決される）

- [ ] **Step 2: 失敗するテストを書く**

`src-tauri/src/usecase/registry.rs` の tests に追加（tests モジュールが無ければ作る）:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::cases;

    #[test]
    fn test_names_is_sorted_and_contains_known_cases() {
        let mut reg = Registry::new();
        cases::register_all(&mut reg);
        let names = reg.names();

        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "names() はソート済みで返す");

        assert!(names.contains(&"search_mails"));
        assert!(names.contains(&"mark_read"));
    }

    #[test]
    fn test_describe_returns_schema_with_properties() {
        let mut reg = Registry::new();
        cases::register_all(&mut reg);
        let infos = reg.describe();

        let search = infos
            .iter()
            .find(|i| i.name == "search_mails")
            .expect("search_mails が登録されている");
        // SearchMailsInput の account_id / query が schema に現れる
        let props = &search.input_schema["properties"];
        assert!(props.get("account_id").is_some(), "schema: {}", search.input_schema);
        assert!(props.get("query").is_some());
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib usecase::registry 2>&1 | tail -20`
Expected: FAIL `no method named names`

- [ ] **Step 4: trait に schema フックを追加**

`src-tauri/src/usecase/traits.rs`。`UseCase` の `Input` 境界に `JsonSchema` を追加:

```rust
#[async_trait::async_trait]
pub trait UseCase: Send + Sync {
    type Input: DeserializeOwned + Send + schemars::JsonSchema;
    type Output: Serialize;

    fn name(&self) -> &'static str;
    fn risk(&self, input: &Self::Input, ctx: &Ctx) -> Result<Risk, AppError>;
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError>;
}
```

`ErasedUseCase` にメソッド追加:

```rust
#[async_trait::async_trait]
pub trait ErasedUseCase: Send + Sync {
    fn name(&self) -> &str;
    fn input_schema(&self) -> Value;
    fn risk_json(&self, input: &Value, ctx: &Ctx) -> Result<Risk, AppError>;
    async fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError>;
}
```

blanket impl に実装追加:

```rust
    fn input_schema(&self) -> Value {
        let schema = schemars::schema_for!(T::Input);
        serde_json::to_value(schema).unwrap_or_else(|_| serde_json::json!({"type": "object"}))
    }
```

注: `unwrap_or_else` でフォールバックしているのは、schema のシリアライズ失敗が実質起こり得ず、かつここで `Result` を返すと呼び出し側が全て煩雑になるため。`unwrap()` は使っていない。

- [ ] **Step 5: 全 Input 型に JsonSchema derive を追加**

`src-tauri/src/usecase/cases/` 配下の**すべての** `#[derive(Deserialize)]` が付いた Input 構造体に `schemars::JsonSchema` を追加する。

Run: `cd src-tauri && grep -rn "derive(Deserialize)" src/usecase/cases/`

各ヒット箇所を次の形に変更:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct SearchMailsInput {
    pub account_id: String,
    pub query: String,
    #[serde(default)]
    pub project_id: Option<String>,
}
```

- [ ] **Step 6: Registry に列挙 API を実装**

`src-tauri/src/usecase/registry.rs`:

```rust
/// レジストリに登録された 1 UseCase の外部公開情報。
/// MCP の tools/list と CLI の `call --list` が共用する。
#[derive(Debug, Clone, serde::Serialize)]
pub struct UseCaseInfo {
    pub name: &'static str,
    pub input_schema: serde_json::Value,
}

impl Registry {
    /// 登録済み UseCase 名を昇順で返す。
    /// HashMap の反復順は非決定的なため、出力の安定のためにソートする。
    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.map.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// 登録済み UseCase の名前と入力スキーマを昇順で返す。
    pub fn describe(&self) -> Vec<UseCaseInfo> {
        self.names()
            .into_iter()
            .filter_map(|name| {
                self.map.get(name).map(|uc| UseCaseInfo {
                    name,
                    input_schema: uc.input_schema(),
                })
            })
            .collect()
    }
}
```

`mod.rs` の re-export に `UseCaseInfo` を追加:

```rust
pub use registry::{Registry, UseCaseInfo};
```

- [ ] **Step 7: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib usecase 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 8: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/usecase/
git commit -m "feat(usecase): Registryに列挙APIとJSON Schema導出を追加

MCPのtools/listとCLIの引数検証が共用する。names()は
HashMapの非決定的な反復順を避けるためソートして返す。"
```

---

### Task 5: read 系 4 つの UseCase 載せ替え

**Files:**
- Modify: `src-tauri/src/usecase/cases/mailbox.rs`
- Modify: `src-tauri/src/usecase/cases/project.rs`
- Modify: `src-tauri/src/commands/mail_commands.rs`
- Modify: `src-tauri/src/commands/project_commands.rs`

**Interfaces:**
- Consumes: `Registry::register`（Task 4 の `JsonSchema` 境界が必要）
- Produces: UseCase 名 `get_threads`, `get_threads_by_project`, `get_unread_counts`, `get_projects`。いずれも `Risk::Read`

**入出力の対応:**

| UseCase | Input | Output |
|---|---|---|
| `get_threads` | `{account_id: String, folder: String}` | `Vec<Thread>` |
| `get_threads_by_project` | `{project_id: String}` | `Vec<Thread>` |
| `get_unread_counts` | `{account_id: String}` | `UnreadCounts` |
| `get_projects` | `{account_id: String}` | `Vec<Project>` |

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/cases/mailbox.rs` の tests に追加:

```rust
#[tokio::test]
async fn test_get_threads_returns_empty_for_unknown_account() {
    let (db, pending, batches, sync_locks) = build_states();
    let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
    let uc = GetThreadsUseCase;
    let input = GetThreadsInput {
        account_id: "nope".into(),
        folder: "INBOX".into(),
    };
    assert_eq!(uc.risk(&input, &ctx).expect("risk"), Risk::Read);
    let out = uc.run(input, &ctx).await.expect("run");
    assert!(out.is_empty());
}
```

`build_states()` は同ファイル内の既存テストヘルパに合わせる。無ければ `context.rs` の tests のものを参考に定義する。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib cases::mailbox 2>&1 | tail -20`
Expected: FAIL `cannot find struct GetThreadsUseCase`

- [ ] **Step 3: mailbox.rs に 3 つの UseCase を実装**

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetThreadsInput {
    pub account_id: String,
    pub folder: String,
}

pub struct GetThreadsUseCase;

#[async_trait::async_trait]
impl UseCase for GetThreadsUseCase {
    type Input = GetThreadsInput;
    type Output = Vec<Thread>;

    fn name(&self) -> &'static str { "get_threads" }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let all_mails = ctx.with_conn(|conn| {
            mails::get_mails_by_account(conn, &input.account_id, &input.folder)
        })?;
        Ok(mails::build_threads(&all_mails))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetThreadsByProjectInput {
    pub project_id: String,
}

pub struct GetThreadsByProjectUseCase;

#[async_trait::async_trait]
impl UseCase for GetThreadsByProjectUseCase {
    type Input = GetThreadsByProjectInput;
    type Output = Vec<Thread>;

    fn name(&self) -> &'static str { "get_threads_by_project" }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| mails::get_threads_by_project(conn, &input.project_id))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetUnreadCountsInput {
    pub account_id: String,
}

pub struct GetUnreadCountsUseCase;

#[async_trait::async_trait]
impl UseCase for GetUnreadCountsUseCase {
    type Input = GetUnreadCountsInput;
    type Output = UnreadCounts;

    fn name(&self) -> &'static str { "get_unread_counts" }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| mails::get_unread_counts(conn, &input.account_id))
    }
}
```

`UnreadCounts` と `Thread` の import を追加する（`mail_commands.rs` の use を参照）。

同ファイルの `register_mailbox_cases` に登録を追加:

```rust
    registry.register(GetThreadsUseCase);
    registry.register(GetThreadsByProjectUseCase);
    registry.register(GetUnreadCountsUseCase);
```

- [ ] **Step 4: project.rs に `get_projects` を実装**

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetProjectsInput {
    pub account_id: String,
}

pub struct GetProjectsUseCase;

#[async_trait::async_trait]
impl UseCase for GetProjectsUseCase {
    type Input = GetProjectsInput;
    type Output = Vec<Project>;

    fn name(&self) -> &'static str { "get_projects" }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::list_projects(conn, &input.account_id))
    }
}
```

`register_project_cases` に `registry.register(GetProjectsUseCase);` を追加。

- [ ] **Step 5: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib cases:: 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 6: Tauri command を dispatch 経由に書き換える**

`src-tauri/src/commands/mail_commands.rs`。**関数名は変えない**。`get_threads` を例に:

```rust
#[tauri::command]
pub async fn get_threads(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    folder: String,
) -> Result<Vec<Thread>, AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "get_threads",
        serde_json::json!({ "account_id": account_id, "folder": folder }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected get_threads output: {e}")))
}
```

`get_threads_by_project`, `get_unread_counts`, `get_projects` も同じ形にする。ペイロードのキー名は各 Input 構造体のフィールド名と一致させること。

- [ ] **Step 7: ビルドと全テストを確認**

Run: `cd src-tauri && cargo test 2>&1 | tail -30`
Expected: PASS

Run: `cd .. && pnpm build 2>&1 | tail -10`
Expected: 成功（frontend の invoke 呼び出しは引数名が同じなので変更不要）

- [ ] **Step 8: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/cases/ src-tauri/src/commands/
git commit -m "feat(usecase): read系4つをdispatchバスへ載せ替え

get_threads / get_threads_by_project / get_unread_counts /
get_projects。いずれもRisk::Readでgateを素通りする。
CLI/MCPから案件・スレッド一覧を参照できるようにするため。"
```

---

### Task 6: `sync_account` の載せ替え

**Files:**
- Create: `src-tauri/src/usecase/cases/sync.rs`
- Modify: `src-tauri/src/usecase/cases/mod.rs`
- Modify: `src-tauri/src/commands/mail_commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `ProgressSink`（Task 3）、`Ctx::sync_locks()`
- Produces: UseCase 名 `sync_account`。Input `{account_id: String}`、Output `u32`（取り込み件数）。`Risk::Reversible`

**設計判断:**
- `SyncLocks` の多重起動ガードを UseCase 側へ移す。ロック取得失敗時は現行どおり `Ok(0)` を返す（エラーではなく「新規取り込みなし」と等価）
- 進捗は `ctx.progress().emit("sync-progress", ...)` に置き換える。イベント名と payload の形は現行の `SyncProgressEvent` を維持し、frontend を変更しない
- `spawn_embedding_pass(app)` は UseCase の外に残し、Tauri command 側で dispatch 成功後に呼ぶ

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/cases/sync.rs` を新規作成し、tests を含める:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::progress::RecordingProgressSink;

    #[tokio::test]
    async fn test_sync_returns_zero_when_lock_is_held() {
        let (db, pending, batches, sync_locks) = build_states();
        // 別の同期が進行中の状況を作る
        assert!(sync_locks.try_begin("acct-1"));

        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let uc = SyncAccountUseCase;
        let out = uc
            .run(SyncAccountInput { account_id: "acct-1".into() }, &ctx)
            .await
            .expect("run");
        assert_eq!(out, 0, "進行中なら 0 件を返す（エラーにしない）");
    }

    #[tokio::test]
    async fn test_sync_risk_is_reversible() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = SyncAccountInput { account_id: "acct-1".into() };
        assert_eq!(SyncAccountUseCase.risk(&input, &ctx).expect("risk"), Risk::Reversible);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib cases::sync 2>&1 | tail -20`
Expected: FAIL（モジュール未登録 / 型未定義）

- [ ] **Step 3: UseCase を実装**

`src-tauri/src/usecase/cases/sync.rs`:

```rust
use serde::Deserialize;

use crate::context::Ctx;
use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::sync_service;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SyncAccountInput {
    pub account_id: String,
}

pub struct SyncAccountUseCase;

#[async_trait::async_trait]
impl UseCase for SyncAccountUseCase {
    type Input = SyncAccountInput;
    type Output = u32;

    fn name(&self) -> &'static str { "sync_account" }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        // 同一アカウントの同期が進行中なら開始しない（画面遷移等での多重起動対策）。
        // エラーではなく 0 件を返す: 呼び出し側にとって「新規取り込みなし」と等価
        if !ctx.sync_locks().try_begin(&input.account_id) {
            return Ok(0);
        }
        let result = run_locked(&input.account_id, ctx).await;
        ctx.sync_locks().finish(&input.account_id);
        result
    }
}

async fn run_locked(account_id: &str, ctx: &Ctx<'_>) -> Result<u32, AppError> {
    let account = ctx.with_conn(|conn| accounts::get_account(conn, account_id))?;
    let secure_store = ctx.secure_store()?;
    sync_service::sync_account(
        ctx.db(),
        &account,
        || crate::commands::mail_commands::resolve_imap_credentials(&account, secure_store),
        |done, total| {
            ctx.progress().emit(
                "sync-progress",
                &serde_json::json!({
                    "account_id": account_id,
                    "done": done,
                    "total": total,
                }),
            );
        },
    )
    .await
}

pub fn register_sync_cases(registry: &mut Registry) {
    registry.register(SyncAccountUseCase);
}
```

注: `resolve_imap_credentials` が `mail_commands.rs` で private なら `pub(crate)` に変更する。所在は `grep -rn "fn resolve_imap_credentials" src/` で確認すること。より適切な置き場所（`mail_sync` 配下）があればそちらへ移してよい。

- [ ] **Step 4: mod.rs に登録**

`src-tauri/src/usecase/cases/mod.rs`:

```rust
pub mod sync;
```

`register_all` に追加:

```rust
    sync::register_sync_cases(registry);
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib cases::sync 2>&1 | tail -20`
Expected: PASS（2 テスト）

- [ ] **Step 6: GUI 用の ProgressSink を実装**

`src-tauri/src/lib.rs`（または新規 `src-tauri/src/tauri_progress.rs`）に追加:

```rust
use tauri::{AppHandle, Emitter};
use crate::usecase::ProgressSink;

/// Tauri のイベントとして進捗を発行する ProgressSink。GUI driver 用。
pub struct TauriProgressSink {
    app: AppHandle,
}

impl TauriProgressSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ProgressSink for TauriProgressSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        // 進捗はベストエフォート（emit 失敗で本処理は止めない）
        let _ = self.app.emit(event, payload);
    }
}
```

- [ ] **Step 7: Tauri command を書き換える**

`src-tauri/src/commands/mail_commands.rs` の `sync_account`:

```rust
#[tauri::command]
pub async fn sync_account(
    app: AppHandle,
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
) -> Result<u32, AppError> {
    let sink = crate::TauriProgressSink::new(app.clone());
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks)
        .with_progress(&sink);
    let out = dispatch(
        &registry,
        "sync_account",
        serde_json::json!({ "account_id": account_id }),
        &ctx,
    )
    .await?;
    let count: u32 = serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected sync_account output: {e}")))?;
    // 埋め込み生成は GUI プロセスのみで起動する（UseCase の外に残す依存）
    spawn_embedding_pass(&app);
    Ok(count)
}
```

旧 `sync_account_locked` は UseCase 側へ移ったので削除する。他から参照されていないことを `grep -rn "sync_account_locked" src/` で確認すること。

- [ ] **Step 8: ビルドと全テストを確認**

Run: `cd src-tauri && cargo test 2>&1 | tail -30`
Expected: PASS

- [ ] **Step 9: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/cases/ src-tauri/src/commands/mail_commands.rs src-tauri/src/lib.rs
git commit -m "feat(usecase): sync_accountをdispatchバスへ載せ替え

SyncLocksの多重起動ガードをUseCase側へ移動。進捗は
ProgressSink経由に変更し、イベント名とpayloadは維持して
frontendを変更しない。spawn_embedding_passはAppHandle依存
のためcommand側に残す。"
```

---

### Task 7: `classify_batch` の載せ替え

**Files:**
- Create: `src-tauri/src/usecase/cases/classify.rs`
- Modify: `src-tauri/src/usecase/cases/mod.rs`
- Modify: `src-tauri/src/commands/classify_commands.rs`

**Interfaces:**
- Consumes: `ProgressSink`（Task 3）、`Ctx::pending()`, `Ctx::batches()`, `Ctx::db()`
- Produces: UseCase 名 `classify_batch`。Input `{account_id: String}`、Output `ClassifyBatchOutcome`。`Risk::Reversible`

**設計判断:** `ClassifyBatchOutcome` は `Serialize` が必要。既に derive されているか確認し、無ければ追加する。分類は案件の割り当てを変えるだけで取り消せるため `Reversible`。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/usecase/cases/classify.rs` を新規作成:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_classify_batch_risk_is_reversible() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = ClassifyBatchInput { account_id: "acct-1".into() };
        assert_eq!(
            ClassifyBatchUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Reversible
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib cases::classify 2>&1 | tail -20`
Expected: FAIL（型未定義）

- [ ] **Step 3: UseCase を実装**

```rust
use serde::Deserialize;

use crate::classifier::{build_classifier, service, ClassifyBatchOutcome};
use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ClassifyBatchInput {
    pub account_id: String,
}

pub struct ClassifyBatchUseCase;

#[async_trait::async_trait]
impl UseCase for ClassifyBatchUseCase {
    type Input = ClassifyBatchInput;
    type Output = ClassifyBatchOutcome;

    fn name(&self) -> &'static str { "classify_batch" }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        // 案件割り当ての変更は取り消せる
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let secure_store = ctx.secure_store()?;
        let classifier = ctx.with_conn(|conn| build_classifier(conn, secure_store))?;
        let account_id = input.account_id;
        service::classify_batch(
            &ctx.db().0,
            classifier.as_ref(),
            ctx.pending(),
            ctx.batches(),
            &account_id,
            |current, total, assigned_mail_id| {
                ctx.progress().emit(
                    "classify-progress",
                    &serde_json::json!({
                        "account_id": account_id,
                        "current": current,
                        "total": total,
                        "assigned_mail_id": assigned_mail_id,
                    }),
                );
            },
        )
        .await
    }
}

pub fn register_classify_cases(registry: &mut Registry) {
    registry.register(ClassifyBatchUseCase);
}
```

`build_classifier` の正確なパスは `grep -rn "fn build_classifier" src/` で確認すること。`classify_commands.rs` で private なら `pub(crate)` にするか、`classifier` モジュール側へ移す。

- [ ] **Step 4: mod.rs に登録**

```rust
pub mod classify;
```

`register_all` に `classify::register_classify_cases(registry);` を追加。

- [ ] **Step 5: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib cases::classify 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 6: Tauri command を書き換える**

`src-tauri/src/commands/classify_commands.rs`:

```rust
#[tauri::command]
pub async fn classify_batch(
    app: AppHandle,
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
) -> Result<ClassifyBatchOutcome, AppError> {
    let sink = crate::TauriProgressSink::new(app);
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks)
        .with_progress(&sink);
    let out = dispatch(
        &registry,
        "classify_batch",
        serde_json::json!({ "account_id": account_id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected classify_batch output: {e}")))
}
```

`ClassifyBatchOutcome` に `Deserialize` が無ければ derive を追加する（dispatch の戻り値 `Value` から復元するため）。

- [ ] **Step 7: ビルドと全テストを確認**

Run: `cd src-tauri && cargo test 2>&1 | tail -30`
Expected: PASS

- [ ] **Step 8: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/usecase/cases/ src-tauri/src/commands/classify_commands.rs
git commit -m "feat(usecase): classify_batchをdispatchバスへ載せ替え

進捗をProgressSink経由に変更。イベント名とpayloadは維持。
BACKLOG 4-5のうちclassify分。"
```

---

### Task 8: TTY 判定

**Files:**
- Create: `src-tauri/src/cli/mod.rs`
- Create: `src-tauri/src/cli/tty.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `Driver`（Task 1）
- Produces:
  - `fn driver_for(is_tty: bool) -> Driver`（純関数。テスト対象）
  - `fn detect_stdin_tty() -> bool`（実環境の判定）
  - `fn current_driver() -> Driver`（上記 2 つの合成）

**設計判断:** stdout ではなく **stdin** で判定する。利用者が `pigeon-cli search ... | jq` のように出力をパイプしても、stdin は端末のままなので人間の操作と正しく判定される。stdout で判定するとこのケースが誤って `CliAutomated` に落ちる。

判定の実装には `std::io::IsTerminal`（Rust 1.70+ 標準）を使う。外部 crate は不要。

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/cli/tty.rs` を新規作成:

```rust
use crate::usecase::Driver;

/// TTY 判定の結果から driver を決める純関数。
///
/// stdin で判定するのは、利用者が出力をパイプしても
/// （`pigeon-cli search ... | jq`）stdin は端末のまま残るため。
/// stdout で判定するとこのケースを誤って非対話と見なす。
pub fn driver_for(is_tty: bool) -> Driver {
    if is_tty {
        Driver::CliInteractive
    } else {
        Driver::CliAutomated
    }
}

/// 実環境の stdin が端末に接続されているかを返す。
pub fn detect_stdin_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// 実環境から driver を決める。
pub fn current_driver() -> Driver {
    driver_for(detect_stdin_tty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tty_maps_to_interactive() {
        assert_eq!(driver_for(true), Driver::CliInteractive);
    }

    #[test]
    fn test_non_tty_maps_to_automated() {
        // エージェント経由の起動（Claude Code の Bash ツール等）はここに落ちる
        assert_eq!(driver_for(false), Driver::CliAutomated);
    }
}
```

- [ ] **Step 2: cli モジュールを登録**

`src-tauri/src/cli/mod.rs` を新規作成:

```rust
pub mod tty;
```

`src-tauri/src/lib.rs` に追加:

```rust
pub mod cli;
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib cli::tty 2>&1 | tail -20`
Expected: PASS（2 テスト）

- [ ] **Step 4: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/cli/ src-tauri/src/lib.rs
git commit -m "feat(cli): stdinのTTY有無でdriverを決める

判定をdriver_for(bool)の純関数に切り出し、実環境の検出と
分離してテスト可能にする。stdoutではなくstdinで判定するのは
出力をパイプしても人間の操作と判定するため。"
```

---

### Task 9: CLI ランタイム（DB/SecureStore を開く + 排他制御）

**Files:**
- Create: `src-tauri/src/cli/runtime.rs`
- Modify: `src-tauri/src/cli/mod.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: `Ctx::new_headless`（Task 2）、`cli::tty::current_driver`（Task 8）
- Produces:
  - `struct CliRuntime { db: DbState, secure_store: SecureStore, pending: PendingClassifications, batches: ClassifyBatches, sync_locks: SyncLocks, registry: Registry, driver: Driver }`
  - `fn CliRuntime::open() -> Result<CliRuntime, AppError>`
  - `fn CliRuntime::ctx(&self) -> Ctx<'_>`

**設計判断:** GUI と DB/Stronghold を共有できないため、CLI 起動時にロックを取れなければ明示的なエラーで終了する。SQLite は同一ファイルを複数プロセスで開けてしまうため「開けたか」では検出できない。**Stronghold のスナップショットファイルのロック取得失敗**を検出点にする。Stronghold が排他ロックを取らない場合に備え、`PRAGMA locking_mode` ではなく、DB ファイルと同じディレクトリに CLI 専用のロックファイルを作る方式でもよい。

実装前に確認: GUI 起動中に CLI から同じ Stronghold を開こうとするとどうなるか（エラーになるか、黙って壊れるか）。**黙って壊れる場合はロックファイル方式を必ず採ること。**

- [ ] **Step 1: clap を追加**

`src-tauri/Cargo.toml` の `[dependencies]`:

```toml
clap = { version = "4", features = ["derive"] }
```

`[[bin]]` セクションを追加:

```toml
[[bin]]
name = "pigeon-cli"
path = "src/bin/pigeon-cli.rs"
```

- [ ] **Step 2: GUI 同時起動時の挙動を実機確認**

Run: GUI（`pnpm tauri dev`）を起動した状態で、別ターミナルから最小の Rust テストプログラムか既存のテストで Stronghold を開いてみる。

確認すること:
- Stronghold の open がエラーを返すか
- エラーを返さない場合、スナップショットが破損しないか

**この結果に応じて Step 3 の実装方針を決める。** 結果を作業ログかコミットメッセージに残すこと。

- [ ] **Step 3: ランタイムを実装**

`src-tauri/src/cli/runtime.rs`:

```rust
use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::error::AppError;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SyncLocks};
use crate::usecase::{cases, Driver, Registry};

/// CLI / MCP プロセスが持つ実行環境。GUI の Tauri State 群に相当する。
pub struct CliRuntime {
    db: DbState,
    secure_store: SecureStore,
    pending: PendingClassifications,
    batches: ClassifyBatches,
    sync_locks: SyncLocks,
    registry: Registry,
    driver: Driver,
}

impl CliRuntime {
    /// DB と SecureStore を開き、UseCase レジストリを構築する。
    /// GUI が起動中で排他できない場合はエラーを返す。
    pub fn open(driver: Driver) -> Result<Self, AppError> {
        // lib.rs の run() が行っている初期化と同じ手順を踏む。
        // DB パス・マイグレーション・マスタ鍵解決の実装は lib.rs を参照して合わせること。
        let db = open_db()?;
        let secure_store = open_secure_store()?;

        let registry = {
            let mut reg = Registry::new();
            cases::register_all(&mut reg);
            reg
        };

        Ok(Self {
            db,
            secure_store,
            pending: PendingClassifications::new(),
            batches: ClassifyBatches::new(),
            sync_locks: SyncLocks::new(),
            registry,
            driver,
        })
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new_headless(
            &self.db,
            &self.secure_store,
            &self.pending,
            &self.batches,
            &self.sync_locks,
            self.driver,
        )
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}
```

`open_db()` と `open_secure_store()` は `lib.rs` の `run()` 内にある初期化処理を関数として切り出して共用する。**`lib.rs` から重複コピーせず、共通関数に括り出して両者から呼ぶこと。** 切り出し先は `src-tauri/src/state.rs` か新規 `src-tauri/src/bootstrap.rs` が適切。

GUI 同時起動の検出は Step 2 の結果に応じて `open()` の冒頭に実装し、失敗時は次のエラーを返す:

```rust
return Err(AppError::Validation(
    "Pigeon が起動中のため CLI から実行できません。アプリを終了してから再実行してください。".into(),
));
```

- [ ] **Step 4: 最小のエントリポイントを作る**

`src-tauri/src/bin/pigeon-cli.rs`:

```rust
use pigeon_lib::cli::{runtime::CliRuntime, tty};

#[tokio::main]
async fn main() {
    let driver = tty::current_driver();
    let runtime = match CliRuntime::open(driver) {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // Task 10 でサブコマンド処理に差し替える
    println!("driver={} usecases={}", driver.as_str(), runtime.registry().names().len());
}
```

- [ ] **Step 5: ビルドと動作確認**

Run: `cd src-tauri && cargo build --bin pigeon-cli 2>&1 | tail -10`
Expected: 成功

Run: `cd src-tauri && ./target/debug/pigeon-cli`
Expected: `driver=cli_interactive usecases=25` のような出力（件数は登録数に依存）

Run: `cd src-tauri && ./target/debug/pigeon-cli | cat`
Expected: `driver=cli_automated ...`（パイプすると stdin が端末のままなので **cli_interactive のまま**。この確認は `echo | ./target/debug/pigeon-cli` で行う）

Run: `cd src-tauri && echo | ./target/debug/pigeon-cli`
Expected: `driver=cli_automated ...`

- [ ] **Step 6: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/cli/ src-tauri/src/bin/ src-tauri/src/lib.rs
git commit -m "feat(cli): pigeon-cliバイナリとランタイムを追加

DB/SecureStoreの初期化をlib.rsと共用し、GUI起動中は
明示的なエラーで終了する。DBは単一Mutex<Connection>で
Strongholdはファイルロックを取るため共存できない。"
```

---

### Task 10: `call` 汎用ディスパッチと出力整形

**Files:**
- Create: `src-tauri/src/cli/output.rs`
- Modify: `src-tauri/src/bin/pigeon-cli.rs`
- Modify: `src-tauri/src/cli/mod.rs`

**Interfaces:**
- Consumes: `CliRuntime`（Task 9）、`Registry::describe`（Task 4）
- Produces:
  - `fn render(value: &Value, as_json: bool) -> String`
  - CLI: `pigeon-cli call <name> <json>`、`pigeon-cli call --list`

**設計判断:** `call` はバスに載った UseCase を名前で直接叩ける汎用口。サブコマンドの定義漏れが機能の欠落にならない。出力は `--json` で生の JSON、既定では人間向けに整形する。

- [ ] **Step 1: 出力整形の失敗テストを書く**

`src-tauri/src/cli/output.rs` を新規作成:

```rust
use serde_json::Value;

/// dispatch の戻り値を表示用の文字列にする。
/// as_json なら整形済み JSON、そうでなければ人間向けの要約。
pub fn render(value: &Value, as_json: bool) -> String {
    if as_json {
        return serde_json::to_string_pretty(value)
            .unwrap_or_else(|_| value.to_string());
    }
    render_human(value)
}

fn render_human(value: &Value) -> String {
    match value {
        Value::Null => "(no output)".to_string(),
        Value::Array(items) if items.is_empty() => "(empty)".to_string(),
        Value::Array(items) => items
            .iter()
            .map(render_line)
            .collect::<Vec<_>>()
            .join("\n"),
        other => render_line(other),
    }
}

/// 1 要素を 1 行にする。id / name / subject など代表的なキーを優先して拾う。
fn render_line(value: &Value) -> String {
    let Value::Object(map) = value else {
        return value.to_string();
    };
    for key in ["subject", "name", "title", "id"] {
        if let Some(Value::String(s)) = map.get(key) {
            return s.clone();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_mode_is_pretty_printed() {
        let v = serde_json::json!({"a": 1});
        let out = render(&v, true);
        assert!(out.contains('\n'), "整形されている: {out}");
    }

    #[test]
    fn test_empty_array_is_reported() {
        assert_eq!(render(&serde_json::json!([]), false), "(empty)");
    }

    #[test]
    fn test_null_is_reported() {
        assert_eq!(render(&Value::Null, false), "(no output)");
    }

    #[test]
    fn test_array_of_objects_uses_representative_key() {
        let v = serde_json::json!([
            {"id": "1", "subject": "hello"},
            {"id": "2", "name": "world"}
        ]);
        assert_eq!(render(&v, false), "hello\nworld");
    }
}
```

- [ ] **Step 2: テストが通ることを確認**

`src-tauri/src/cli/mod.rs` に `pub mod output;` を追加してから:

Run: `cd src-tauri && cargo test --lib cli::output 2>&1 | tail -20`
Expected: PASS（4 テスト）

- [ ] **Step 3: CLI に call サブコマンドを実装**

`src-tauri/src/bin/pigeon-cli.rs`:

```rust
use clap::{Parser, Subcommand};
use pigeon_lib::cli::{output, runtime::CliRuntime, tty};
use pigeon_lib::usecase::dispatch;

#[derive(Parser)]
#[command(name = "pigeon-cli", about = "Pigeon をコマンドラインから操作する")]
struct Cli {
    /// 結果を JSON で出力する
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// UseCase を名前で直接呼ぶ
    Call {
        /// 登録済み UseCase 名。--list で一覧
        name: Option<String>,
        /// 入力 JSON（例: '{"account_id":"a1"}'）
        #[arg(default_value = "{}")]
        input: String,
        /// 呼べる UseCase 名と入力スキーマを一覧する
        #[arg(long)]
        list: bool,
    },
    /// MCP サーバーを stdio で起動する
    Mcp,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let driver = tty::current_driver();
    let runtime = CliRuntime::open(driver).map_err(|e| e.to_string())?;

    match cli.command {
        Commands::Call { list: true, .. } => {
            let infos = runtime.registry().describe();
            let value = serde_json::to_value(&infos).map_err(|e| e.to_string())?;
            if cli.json {
                println!("{}", output::render(&value, true));
            } else {
                for info in infos {
                    println!("{}", info.name);
                }
            }
            Ok(())
        }
        Commands::Call { name, input, .. } => {
            let name = name.ok_or_else(|| {
                "UseCase 名を指定してください（一覧は --list）".to_string()
            })?;
            let input: serde_json::Value =
                serde_json::from_str(&input).map_err(|e| format!("入力 JSON が不正です: {e}"))?;
            let ctx = runtime.ctx();
            let out = dispatch(runtime.registry(), &name, input, &ctx)
                .await
                .map_err(|e| e.to_string())?;
            println!("{}", output::render(&out, cli.json));
            Ok(())
        }
        Commands::Mcp => Err("MCP サーバーは未実装です".to_string()),
    }
}
```

- [ ] **Step 4: 動作確認**

Run: `cd src-tauri && cargo build --bin pigeon-cli 2>&1 | tail -5`
Expected: 成功

Run: `cd src-tauri && ./target/debug/pigeon-cli call --list`
Expected: `search_mails` などの UseCase 名が 1 行ずつ、昇順で出力される

Run: `cd src-tauri && ./target/debug/pigeon-cli call --list --json | head -20`
Expected: `name` と `input_schema` を持つ JSON 配列

Run: `cd src-tauri && ./target/debug/pigeon-cli call get_projects '{"account_id":"存在しないID"}'`
Expected: `(empty)` またはエラーメッセージ（DB の状態による）

Run: `cd src-tauri && ./target/debug/pigeon-cli call unknown_case '{}'`
Expected: `error: ... unknown use case: unknown_case`、終了コード 1

Run: `cd src-tauri && ./target/debug/pigeon-cli call unknown_case '{}'; echo "exit=$?"`
Expected: `exit=1`

- [ ] **Step 5: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/cli/ src-tauri/src/bin/
git commit -m "feat(cli): call汎用ディスパッチと出力整形を追加

バスに載ったUseCaseはサブコマンド定義なしで即座に叩ける。
--listで名前とJSON Schemaを一覧。--jsonで機械可読出力。"
```

---

### Task 11: 名前付きサブコマンド

**Files:**
- Modify: `src-tauri/src/bin/pigeon-cli.rs`

**Interfaces:**
- Consumes: Task 10 の `run()` 構造
- Produces: `pigeon-cli sync`, `search`, `projects`, `threads`, `unread`

**設計判断:** 頻用操作に読みやすい名前を与える。内部では `call` と同じく `dispatch` を呼ぶだけ。`--help` に載ることで発見性が上がる。全 UseCase にサブコマンドを与えることはしない（YAGNI。`call` があるため）。

- [ ] **Step 1: サブコマンドを追加**

`Commands` enum に追加:

```rust
    /// アカウントのメールを同期する
    Sync {
        account_id: String,
    },
    /// メールを全文検索する
    Search {
        account_id: String,
        query: String,
        /// 案件で絞り込む
        #[arg(long)]
        project_id: Option<String>,
    },
    /// 案件一覧を表示する
    Projects {
        account_id: String,
    },
    /// スレッド一覧を表示する
    Threads {
        account_id: String,
        #[arg(default_value = "INBOX")]
        folder: String,
    },
    /// 未読件数を表示する
    Unread {
        account_id: String,
    },
```

- [ ] **Step 2: ディスパッチ処理を追加**

`run()` の match に追加。共通処理を関数に括り出す:

```rust
async fn call_and_print(
    runtime: &CliRuntime,
    name: &str,
    input: serde_json::Value,
    as_json: bool,
) -> Result<(), String> {
    let ctx = runtime.ctx();
    let out = dispatch(runtime.registry(), name, input, &ctx)
        .await
        .map_err(|e| e.to_string())?;
    println!("{}", output::render(&out, as_json));
    Ok(())
}
```

各アーム:

```rust
        Commands::Sync { account_id } => {
            call_and_print(
                &runtime,
                "sync_account",
                serde_json::json!({ "account_id": account_id }),
                cli.json,
            )
            .await
        }
        Commands::Search { account_id, query, project_id } => {
            call_and_print(
                &runtime,
                "search_mails",
                serde_json::json!({
                    "account_id": account_id,
                    "query": query,
                    "project_id": project_id,
                }),
                cli.json,
            )
            .await
        }
        Commands::Projects { account_id } => {
            call_and_print(
                &runtime,
                "get_projects",
                serde_json::json!({ "account_id": account_id }),
                cli.json,
            )
            .await
        }
        Commands::Threads { account_id, folder } => {
            call_and_print(
                &runtime,
                "get_threads",
                serde_json::json!({ "account_id": account_id, "folder": folder }),
                cli.json,
            )
            .await
        }
        Commands::Unread { account_id } => {
            call_and_print(
                &runtime,
                "get_unread_counts",
                serde_json::json!({ "account_id": account_id }),
                cli.json,
            )
            .await
        }
```

- [ ] **Step 3: CLI の進捗表示を実装**

`src-tauri/src/cli/progress.rs` を新規作成:

```rust
use std::io::Write;

use serde_json::Value;

use crate::usecase::ProgressSink;

/// 進捗を stderr に出す ProgressSink。stdout は結果専用に保つため。
pub struct StderrProgressSink;

impl ProgressSink for StderrProgressSink {
    fn emit(&self, event: &str, payload: &Value) {
        let line = match (payload.get("done"), payload.get("total")) {
            (Some(done), Some(total)) => format!("{event}: {done}/{total}"),
            _ => match (payload.get("current"), payload.get("total")) {
                (Some(cur), Some(total)) => format!("{event}: {cur}/{total}"),
                _ => format!("{event}"),
            },
        };
        // 進捗はベストエフォート
        let mut err = std::io::stderr();
        let _ = writeln!(err, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_does_not_panic_on_unexpected_payload() {
        StderrProgressSink.emit("sync-progress", &serde_json::json!({"unexpected": true}));
    }
}
```

`src-tauri/src/cli/mod.rs` に `pub mod progress;` を追加。

`CliRuntime::ctx()` を進捗付きに変更するか、`ctx_with_progress(&self, sink: &'a dyn ProgressSink)` を追加して `Sync` サブコマンドで使う。ライフタイムの都合で `ctx()` 内で sink を作れないため、呼び出し側で sink を持つ形にする:

```rust
        Commands::Sync { account_id } => {
            let sink = pigeon_lib::cli::progress::StderrProgressSink;
            let ctx = runtime.ctx().with_progress(&sink);
            let out = dispatch(
                runtime.registry(),
                "sync_account",
                serde_json::json!({ "account_id": account_id }),
                &ctx,
            )
            .await
            .map_err(|e| e.to_string())?;
            println!("{}", output::render(&out, cli.json));
            Ok(())
        }
```

- [ ] **Step 4: 動作確認**

Run: `cd src-tauri && cargo build --bin pigeon-cli 2>&1 | tail -5`
Expected: 成功

Run: `cd src-tauri && ./target/debug/pigeon-cli --help`
Expected: `sync`, `search`, `projects`, `threads`, `unread`, `call`, `mcp` が列挙される

Run: `cd src-tauri && cargo test --lib cli:: 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/cli/ src-tauri/src/bin/
git commit -m "feat(cli): 頻用操作の名前付きサブコマンドを追加

sync / search / projects / threads / unread。内部はcallと
同じくdispatchを呼ぶだけ。進捗はstderrへ出しstdoutを
結果専用に保つ。"
```

---

### Task 12: MCP stdio サーバー

**Files:**
- Create: `src-tauri/src/mcp/mod.rs`
- Create: `src-tauri/src/mcp/protocol.rs`
- Create: `src-tauri/src/mcp/server.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/bin/pigeon-cli.rs`

**Interfaces:**
- Consumes: `CliRuntime`（Task 9）、`Registry::describe`（Task 4）
- Produces: `async fn serve_stdio(runtime: &CliRuntime) -> Result<(), AppError>`

**設計判断:** MCP SDK crate を使わず、JSON-RPC 2.0 を直接実装する。必要なメソッドは `initialize` / `tools/list` / `tools/call` の 3 つだけで、SDK を入れるより依存が軽い。stdin を 1 行 1 メッセージで読み、stdout に 1 行 1 レスポンスを書く。

**重要:** MCP は stdout をプロトコルに占有するため、**ログや進捗は絶対に stdout へ書かない**。進捗は破棄する（`NoOpProgressSink`）。

driver は `Driver::Mcp` を使う（`CliAutomated` ではない）。MCP 経由であることが監査ログに正しく残るようにするため。

- [ ] **Step 1: プロトコル型の失敗テストを書く**

`src-tauri/src/mcp/protocol.rs` を新規作成:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// 通知（notification）には id が無い
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn failure(id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

/// JSON-RPC 2.0 の標準エラーコード
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INTERNAL_ERROR: i32 = -32603;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_parses_without_id() {
        let raw = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req: JsonRpcRequest = serde_json::from_str(raw).expect("parse");
        assert!(req.id.is_none());
        assert_eq!(req.method, "notifications/initialized");
    }

    #[test]
    fn test_success_response_omits_error_field() {
        let res = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"ok": true}));
        let s = serde_json::to_string(&res).expect("serialize");
        assert!(!s.contains("error"), "{s}");
        assert!(s.contains(r#""id":1"#), "{s}");
    }

    #[test]
    fn test_failure_response_omits_result_field() {
        let res = JsonRpcResponse::failure(serde_json::json!(2), METHOD_NOT_FOUND, "nope".into());
        let s = serde_json::to_string(&res).expect("serialize");
        assert!(!s.contains("result"), "{s}");
        assert!(s.contains("-32601"), "{s}");
    }
}
```

- [ ] **Step 2: テストが通ることを確認**

`src-tauri/src/mcp/mod.rs` を作成:

```rust
pub mod protocol;
pub mod server;
```

`src-tauri/src/lib.rs` に `pub mod mcp;` を追加。

Run: `cd src-tauri && cargo test --lib mcp::protocol 2>&1 | tail -20`
Expected: PASS（3 テスト）

- [ ] **Step 3: tools/list の変換テストを書く**

`src-tauri/src/mcp/server.rs` を新規作成し、まず変換関数とテストを書く:

```rust
use serde_json::Value;

use crate::usecase::UseCaseInfo;

/// UseCase 情報を MCP の tool 定義に変換する。
pub fn to_tool_definition(info: &UseCaseInfo) -> Value {
    serde_json::json!({
        "name": info.name,
        "description": format!("Pigeon use case: {}", info.name),
        "inputSchema": info.input_schema,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition_has_required_mcp_fields() {
        let info = UseCaseInfo {
            name: "search_mails",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }),
        };
        let tool = to_tool_definition(&info);
        assert_eq!(tool["name"], "search_mails");
        assert!(tool["description"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib mcp::server 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: stdio ループを実装**

`src-tauri/src/mcp/server.rs` に追加:

```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cli::runtime::CliRuntime;
use crate::error::AppError;
use crate::mcp::protocol::{JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, METHOD_NOT_FOUND};
use crate::usecase::dispatch;

/// stdio で MCP サーバーを走らせる。
///
/// stdout は JSON-RPC が占有するため、ログ・進捗は一切書かない
/// （進捗は NoOpProgressSink に落ちる）。診断は stderr へ。
pub async fn serve_stdio(runtime: &CliRuntime) -> Result<(), AppError> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("mcp: 不正なリクエストを無視しました: {e}");
                continue;
            }
        };
        // 通知（id なし）には応答しない
        let Some(id) = req.id.clone() else { continue };

        let response = handle_request(runtime, &req, id).await;
        let body = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("mcp: 応答のシリアライズに失敗しました: {e}");
                continue;
            }
        };
        if stdout.write_all(body.as_bytes()).await.is_err()
            || stdout.write_all(b"\n").await.is_err()
            || stdout.flush().await.is_err()
        {
            // クライアントが切断した
            break;
        }
    }
    Ok(())
}

async fn handle_request(
    runtime: &CliRuntime,
    req: &JsonRpcRequest,
    id: serde_json::Value,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "pigeon", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "tools/list" => {
            let tools: Vec<_> = runtime
                .registry()
                .describe()
                .iter()
                .map(to_tool_definition)
                .collect();
            JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let ctx = runtime.ctx();
            match dispatch(runtime.registry(), name, args, &ctx).await {
                Ok(out) => {
                    let text = serde_json::to_string_pretty(&out)
                        .unwrap_or_else(|_| out.to_string());
                    JsonRpcResponse::success(
                        id,
                        serde_json::json!({
                            "content": [{ "type": "text", "text": text }]
                        }),
                    )
                }
                // UseCase のエラーは isError で返す（プロトコルエラーではない）
                Err(e) => JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": e.to_string() }],
                        "isError": true
                    }),
                ),
            }
        }
        other => JsonRpcResponse::failure(
            id,
            METHOD_NOT_FOUND,
            format!("未対応のメソッドです: {other}"),
        ),
    }
}
```

注: `INTERNAL_ERROR` を使わない場合は import から外す。

- [ ] **Step 6: CLI から MCP を起動できるようにする**

`src-tauri/src/bin/pigeon-cli.rs` の `Commands::Mcp` アームを差し替える。**driver は `Driver::Mcp` を使う**:

```rust
        Commands::Mcp => {
            // MCP 経由であることを監査ログに残すため CliAutomated ではなく Mcp を使う
            let runtime = CliRuntime::open(pigeon_lib::usecase::Driver::Mcp)
                .map_err(|e| e.to_string())?;
            pigeon_lib::mcp::server::serve_stdio(&runtime)
                .await
                .map_err(|e| e.to_string())
        }
```

これに伴い `run()` の冒頭で無条件に `CliRuntime::open(driver)` している箇所を、Mcp アームより後ろ、あるいは各アームで開く形に整理する。**MCP のときに CLI 用 driver でランタイムを開いてしまわないこと。**

- [ ] **Step 7: 手動で疎通確認**

Run:
```bash
cd src-tauri && printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | ./target/debug/pigeon-cli mcp
```
Expected: 2 行の JSON。1 行目に `serverInfo`、2 行目に `tools` 配列（`search_mails` などを含む）

Run:
```bash
cd src-tauri && printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"unknown/method","params":{}}' \
  | ./target/debug/pigeon-cli mcp
```
Expected: `"code":-32601` を含む応答

- [ ] **Step 8: 全テストとビルドを確認**

Run: `cd src-tauri && cargo test 2>&1 | tail -30`
Expected: PASS

Run: `cd src-tauri && cargo clippy --all-targets 2>&1 | tail -20`
Expected: 警告なし（既存の警告は除く）

- [ ] **Step 9: コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/mcp/ src-tauri/src/bin/ src-tauri/src/lib.rs
git commit -m "feat(mcp): stdio JSON-RPCサーバーを追加

initialize / tools/list / tools/call の3メソッドのみ実装し
SDK依存を避ける。toolsはRegistry::describeから自動導出する
のでUseCaseを載せれば即座に露出する。stdoutはプロトコル
専用でログ・進捗は書かない。"
```

---

### Task 13: ドキュメント更新

**Files:**
- Modify: `docs/BACKLOG.md`
- Modify: `docs/design/2026-07-20-cli-mcp-driver-design.md`
- Modify: `README.md`（存在すれば）

- [ ] **Step 1: BACKLOG を更新**

`docs/BACKLOG.md` の Risk ゲート進捗セクションで、完了項目に取り消し線と完了マークを付ける。

- 4-5 のうち sync / classify が完了、rescan が残っていることを明記
- 5-1 を完了扱いにし、PR 番号を記録
- 5-2 が未着手であること、それにより**エージェントからの Sensitive 操作が完了できない**ことを引き続き明記

- [ ] **Step 2: 使い方を記載**

`docs/design/2026-07-20-cli-mcp-driver-design.md` の末尾に「使い方」節を追加:

```markdown
## 使い方

### CLI

    pigeon-cli --help                              # サブコマンド一覧
    pigeon-cli projects <account_id>               # 案件一覧
    pigeon-cli sync <account_id>                   # 同期（進捗は stderr）
    pigeon-cli search <account_id> "<query>"       # 全文検索
    pigeon-cli call --list                         # 呼べる UseCase と入力スキーマ
    pigeon-cli call <name> '<json>'                # UseCase を直接呼ぶ
    pigeon-cli <任意のコマンド> --json             # 機械可読出力

### MCP

MCP クライアントの設定に次を追加する。

    { "command": "pigeon-cli", "args": ["mcp"] }

tools は Registry から自動導出されるため、UseCase をバスに載せれば
設定を変えずに露出する。
```

- [ ] **Step 3: コミット**

```bash
git add docs/
git commit -m "docs(cli): CLI/MCPの使い方とBACKLOGの進捗を更新"
```

---

## 完了時の検証

すべてのタスク完了後に実行する。

- [ ] `cd src-tauri && cargo test 2>&1 | tail -20` → 全 PASS
- [ ] `cd src-tauri && cargo clippy --all-targets 2>&1 | tail -20` → 新規警告なし
- [ ] `cd src-tauri && cargo fmt --check` → 差分なし
- [ ] `pnpm build` → 成功（frontend が壊れていない）
- [ ] `pnpm test` → 全 PASS
- [ ] GUI を起動し、同期・分類の進捗バーが従来どおり動くことを目視確認
- [ ] GUI 起動中に `pigeon-cli projects <id>` を実行 → 明示的なエラーで終了することを確認
- [ ] GUI 終了後に `pigeon-cli projects <id>` を実行 → 案件一覧が出ることを確認
- [ ] `echo | pigeon-cli call send_mail '<有効なjson>'` → 「approval required」で拒否されることを確認（非対話は承認キュー行き）
- [ ] 対話端末で `pigeon-cli call send_mail '<有効なjson>'` → 承認なしで実行されることを確認（TTY は Ui 相当）

最後の 2 項目が、この設計の中核（TTY による人間/エージェントの区別）が実際に効いているかの確認になる。
