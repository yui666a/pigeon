# Phase 4-2: UseCase バス + Risk ゲート骨格 設計

作成日: 2026-07-14
ステータス: 設計合意済み（実装計画は別途）
親設計: docs/design/2026-07-14-ai-native-mcp-architecture-design.md
前提: Phase 4-1（Ctx 導入）完了・main マージ済み（HEAD cf61661）

## 目的とスコープ

親設計の Phase 4-2 を実装する。3 driver（commands / MCP / 常駐エージェント）が
1 つの dispatch バスを共有するための **UseCase trait + レジストリ + dispatch
バスの骨格**を作り、**read 系のみ**を `Risk::Read` で載せてパターンを確立する。

Phase 4-1 が Ctx を「2 コマンドでパターン確立に留めた」のと同じ精神で、4-2 も
「read 系 UseCase を 1 つだけ載せてバスのパターンを確立」する。全 command の
bus 載せ替えは水平展開（スコープ外）。

### このフェーズでやること

- `UseCase` trait（型安全・同期）と `ErasedUseCase`（消去層・自動導出）の 2 層構造
- レジストリ（`name → Box<dyn ErasedUseCase>`）
- `Risk::{Read, Reversible, Sensitive}` enum
- `Driver::{Ui, Mcp, Agent}` enum と Ctx への driver 情報追加
- `dispatch(name, input, ctx)`: risk 判定 → gate → audit 呼び出し点 → run
- gate: Read=通過 / Reversible・Sensitive=拒否（未実装エラー）
- `AuditSink` trait と in-memory / no-op 実装（記録の実体は 4-4）
- read 系 UseCase を 1 つ実装しレジストリ登録（`SearchMailsUseCase`）
- 引き継ぎ課題 2 の整合（context.rs のフェーズコメント修正）

### このフェーズでやらないこと（スコープ外）

| 項目 | 送り先 |
|------|--------|
| send / delete / archive / flag の UseCase 抽出 | 4-3 |
| Sensitive ゲート本体（承認キューへの投入ロジック） | 4-4 |
| 監査ログの SQLite 永続化（テーブル・migration・SqliteAuditSink） | 4-4 |
| 承認キュー（テーブル・投入・UI） | 4-4 / 5-2 |
| classify / sync / rescan の bus 載せ替え | 4-5 |
| classify_one の service(&Ctx) 化・二重経路解消（引き継ぎ課題 1） | 4-5 |
| async UseCase 対応（trait の async 化） | 4-5 |
| driver 分岐の中身（MCP の Sensitive 拒否など） | Phase 5 |
| 全 command の bus 載せ替え | 水平展開 |

## 現状分析（実装調査に基づく）

Rust バックエンドの command 層を調査し、read 系が最も素直に UseCase 化できると
確認した。

| 分類 | 代表コマンド | 現状の依存 | 対応 Risk |
|------|-------------|-----------|-----------|
| **Read** | `search_mails`(Ctx 済), `get_mails_by_project`, `get_threads`, `get_threads_by_project`, `get_unread_counts`, `get_unclassified_mails`, `get_unclassified_threads`, `get_recent_unread_subjects` | `DbState` のみ（一部 pending） | Read |
| **Reversible** | `set_flagged`, `mark_unread`, `mark_read`, `move_mail`, `unarchive_mail` | `DbState` + `AppHandle`（IMAP はバックグラウンド spawn） | Reversible |
| **Sensitive** | `delete_mail`, `archive_mail`, `send_mail` | `State<DbState>` を直接受ける `*_inner`, SecureStore | Sensitive（archive はプラン依存） |

read 系は全て `db.with_conn` で完結し、Ctx 化・UseCase 化が最も素直。4-2 の
「read 系を Risk::Read で載せてパターン確立」と一致する。

### Phase 4-1 の到達点（前提）

- `src-tauri/src/context.rs` に `Ctx<'a>` が存在。managed State（DbState /
  SecureStore / PendingClassifications / ClassifyBatches / SyncLocks）への
  参照を束ね、`with_conn` / `with_conn_mut` / `secure_store()` / `pending()` /
  `batches()` / `sync_locks()` を提供。
- `search_mails`, `classify_mail` が Ctx 経由に変換済み。
- `AppError`（error.rs）が全域統一済みで、`run(input, ctx) -> Result<_, AppError>`
  の error 軸は無改修で揃う。

## コンポーネント設計

### 1. 型付け方式 — 2 層 trait + ブランケット実装

ジェネリクス trait は関連型を持つため `dyn` 化できない（`type Input` が
呼び出し側から見えない）。一方 3 driver が name でレジストリを引くには
「型を消した共通の入口」が要る。この 2 つを **2 層 trait** で両立する。

**上層（実装者が書く・型安全・同期）:**

```rust
use serde::{de::DeserializeOwned, Serialize};

pub trait UseCase {
    type Input: DeserializeOwned;
    type Output: Serialize;

    fn name(&self) -> &'static str;

    /// 実効 Risk。input を参照できる（archive のプラン依存 Risk 等）。
    /// 多くの UseCase は input を無視して固定 Risk を返す。
    fn risk(&self, input: &Self::Input) -> Risk;

    fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError>;
}
```

**下層（消去層・`dyn` 可・ブランケット実装で自動導出）:**

```rust
use serde_json::Value;

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
        let typed: T::Input = serde_json::from_value(input.clone())
            .map_err(|e| AppError::Validation(format!("invalid input for {}: {e}", self.name())))?;
        Ok(self.risk(&typed))
    }

    fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError> {
        let typed: T::Input = serde_json::from_value(input)
            .map_err(|e| AppError::Validation(format!("invalid input for {}: {e}", self.name())))?;
        let output = self.run(typed, ctx)?;
        serde_json::to_value(output)
            .map_err(|e| AppError::Validation(format!("failed to serialize output: {e}")))
    }
}
```

実装者は型安全な `UseCase` だけを書き、レジストリ・MCP 自動導出・3 driver 共有は
消去層が担保する。`risk_json` が input を deserialize するのは、`risk()` が
input 依存（プラン依存 Risk）だから。JSON パースエラーは既存の
`AppError::Validation` に載せる（新バリアント追加なし・YAGNI）。

**設計判断: `run` は同期で確定する。** 4-2 で載せる read 系は全て同期。async 化は
実際に async UseCase（classify / sync）を載せる 4-5 で導入する（async_trait
または run の async 化）。載せる use case が実在しない段階で async を先決めするのは
YAGNI。

### 2. レジストリ

```rust
use std::collections::HashMap;

pub struct Registry {
    map: HashMap<&'static str, Box<dyn ErasedUseCase>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn register<T: UseCase + 'static>(&mut self, uc: T) {
        self.map.insert(uc.name(), Box::new(uc));
    }

    pub fn lookup(&self, name: &str) -> Option<&dyn ErasedUseCase> {
        self.map.get(name).map(|b| b.as_ref())
    }
}
```

`name → Box<dyn ErasedUseCase>` のマップ。MCP の tool 一覧・JSON Schema 自動導出は
将来このレジストリに乗る（Phase 5-1）。4-2 では read 系 UseCase を 1 つ登録する。

### 3. Risk enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// 自由に実行（検索・一覧）
    Read,
    /// 自動実行 + 監査（フラグ・未読戻し・案件移動）— 実装は 4-4
    Reversible,
    /// 人間の承認必須（送信・サーバー削除）— 実装は 4-4
    Sensitive,
}
```

### 4. Driver enum と Ctx への driver 情報

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// 人間の UI 操作（承認済み扱い）
    Ui,
    /// 外部 LLM（MCP 経由）— 判定の中身は Phase 5
    Mcp,
    /// 常駐エージェント — 判定の中身は Phase 5
    Agent,
}
```

Ctx に `driver: Driver` フィールドを追加する。既存の `Ctx::new` は commands から
呼ばれるため `Driver::Ui` 固定で構築する（commands は全て人間の UI 操作）。
`new_for_test` も `Ui` 既定。アクセサ `Ctx::driver(&self) -> Driver` を追加。

これにより `gate::check(risk, ctx.driver())` のシグネチャが 4-2 で確定し、
Phase 5 で MCP / Agent driver を差し込むときにゲート引数を増やす破壊的変更が
発生しない。enum は 3 バリアントで最小（分岐の中身は Phase 5）。

### 5. dispatch（Command バス）

```rust
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

    // 4-2 では Read のみ載るため実質未発火。記録の実体は 4-4。
    if risk != Risk::Read {
        ctx.audit().record(AuditEntry::new(name, risk, ctx.driver()));
    }

    uc.run_json(input, ctx)
}
```

3 driver すべてがこの 1 関数を通る（特権的な裏口なし）。4-2 の呼び出し元は
UI（commands）のみだが、パイプライン形（lookup → risk → gate → audit → run）を
ここで確定させる。

### 6. gate（Risk ゲート）

```rust
pub fn check(risk: Risk, _driver: Driver) -> Result<(), AppError> {
    match risk {
        Risk::Read => Ok(()),
        // 4-4 で承認キュー投入 / driver 分岐に置き換える。
        // 4-2 では read 系しか載らないため実害はない。
        Risk::Reversible | Risk::Sensitive => Err(AppError::Validation(format!(
            "risk gate not yet implemented for {risk:?} (Phase 4-4)"
        ))),
    }
}
```

`driver` は 4-2 では未使用（`_driver`）だが、シグネチャに含めて拡張点を固定する。
Read=通過、それ以外=明示的エラー。承認キュー本体は 4-4。

### 7. AuditSink trait

```rust
pub struct AuditEntry {
    pub use_case: String,
    pub risk: Risk,
    pub driver: Driver,
    // タイムスタンプ・input 概要は 4-4 で SQLite スキーマ確定時に足す
}

pub trait AuditSink: Send + Sync {
    fn record(&self, entry: AuditEntry);
}

/// 4-2 の既定実装。記録を捨てる（テスト用に InMemory も用意）。
pub struct NoOpAuditSink;
impl AuditSink for NoOpAuditSink {
    fn record(&self, _entry: AuditEntry) {}
}
```

Ctx に audit シンク参照（`&dyn AuditSink`）を追加し、`ctx.audit()` で取得する。
4-2 は NoOp / InMemory 実装のみ。SQLite テーブル（migration v15）と
`SqliteAuditSink` は 4-4 で、実際に Reversible / Sensitive を載せるときに導入する。
使われないテーブルを先行導入しない（YAGNI）。

**Ctx への audit シンク注入方法:** commands から Ctx を組むとき、managed State と
同様に `NoOpAuditSink` の参照を渡す。lib.rs で `NoOpAuditSink` を `.manage()` する
（または Ctx が既定で NoOp を指す）。実装計画で詳細を確定する。

### 8. read 系 UseCase の実装例 — SearchMailsUseCase

```rust
#[derive(Deserialize)]
pub struct SearchMailsInput {
    pub account_id: String,
    pub query: String,
}

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
```

これをレジストリに登録し、`dispatch("search_mails", json, ctx)` で叩けることを
検証する（パターン確立の実例）。

**既存 command は当面そのまま残す。** Tauri command `search_mails`（4-1 で Ctx 化
済み）は dispatch と二重経路になるが、全 command の bus 載せ替えは 4-2 の
スコープ外（水平展開）。4-1 が 2 コマンドでパターン確立に留めたのと同じ割り切り。

### 9. 引き継ぎ課題 2 の整合

`context.rs` の doc コメント「Risk ゲート等は Phase 4-4 で載せる」を、ゲートを
導入する本フェーズ（4-2）に合わせて修正する。Risk ゲートの骨格は 4-2、
ゲート本体（承認キュー）と監査永続化は 4-4、という整合を反映する。

## ファイル構成

新規モジュールは `src-tauri/src/usecase/` に置く（commands と横並びの
アプリケーション層）。

```
src-tauri/src/usecase/
├── mod.rs          # pub use。Risk / Driver / UseCase / ErasedUseCase 再エクスポート
├── risk.rs         # Risk enum
├── driver.rs       # Driver enum
├── traits.rs       # UseCase trait + ErasedUseCase + ブランケット実装
├── registry.rs     # Registry
├── gate.rs         # gate::check
├── audit.rs        # AuditEntry / AuditSink / NoOpAuditSink / InMemoryAuditSink
├── dispatch.rs     # dispatch 関数
└── cases/
    └── search.rs   # SearchMailsUseCase（read 系の実例）
```

- **Modify** `src-tauri/src/lib.rs` — `pub mod usecase;` 追加。`NoOpAuditSink` の
  manage（audit シンク注入方法は実装計画で確定）。
- **Modify** `src-tauri/src/context.rs` — `driver: Driver` フィールドと
  `audit` シンク参照を追加、`driver()` / `audit()` アクセサ、`new` / `new_for_test`
  の更新。フェーズコメント修正。

## データフロー

```
（4-2 の唯一の呼び出し元 = UI commands 経由の検証）

test / 将来の driver
      │  dispatch(registry, "search_mails", json!({...}), ctx)
      ▼
  Registry.lookup("search_mails") ──► &dyn ErasedUseCase
      │
      ├─ risk_json(&input)  ──► from_value::<SearchMailsInput> ──► risk() = Read
      ├─ gate::check(Read, Ui) ──► Ok(())
      ├─ (Read なので audit 未発火)
      └─ run_json(input, ctx)
             └─ from_value ──► run() ──► ctx.with_conn(search::search_mails) ──► to_value
      ▼
  Result<Value, AppError>  （Vec<SearchResult> の JSON）
```

## エラーハンドリング

- 全て `AppError`（既存）で統一。新バリアントは追加しない。
- JSON パース失敗 → `AppError::Validation`。
- 未登録 name → `AppError::Validation`。
- Reversible / Sensitive のゲート拒否 → `AppError::Validation`（4-4 で置換）。
- `unwrap()` / `expect()` はテスト以外で使わない（agent.md）。

## テスト戦略（TDD: Red → Green → Refactor）

- **ErasedUseCase ブランケット**: 型付き `UseCase` を実装すると Value 境界で
  叩けること。`from_value` → `run` → `to_value` のラウンドトリップ。不正 JSON で
  `AppError::Validation` になること。
- **Registry**: register / lookup、未登録名で `None`。
- **gate**: `check(Read, _)` = Ok、`check(Reversible, _)` / `check(Sensitive, _)`
  = Err。driver 3 種で分岐が現状同一（Read 通過）であること。
- **dispatch**: 未登録名エラー、Read UseCase が通って正しい Value を返す、
  Reversible/Sensitive をダミー登録するとゲートで弾かれる。
- **AuditSink**: InMemoryAuditSink が record を蓄積すること（4-4 の SQLite 実装の
  差し替え先を用意）。dispatch で Read が record を呼ばないこと。
- **SearchMailsUseCase**: `new_for_test` Ctx で run が既存 `db::search` と同結果。

既存の `search_mails` command 経路のテストは不変（挙動を変えない）。

## セキュリティ

- 新設する Risk ゲートは「実行してよいか（誰が）」の**認可**ポリシー。既存の
  `mail_policy`（実行方法）/ `cloud_policy`（送信可否）とは層が違い、変更しない。
- 4-2 で載せるのは Read のみ。Sensitive の実行経路は gate が明示的に塞ぐため、
  この段階で破壊的操作が bus から漏れることはない。

## 完了条件（Definition of Done）

- `usecase` モジュールに `UseCase` / `ErasedUseCase`（ブランケット）/ `Registry`
  / `Risk` / `Driver` / `gate::check` / `AuditSink`（NoOp/InMemory）/ `dispatch` が
  存在する。
- `SearchMailsUseCase` がレジストリ登録され、`dispatch("search_mails", …)` で
  既存 search と同結果を返す。
- `gate::check` が Read=通過、Reversible/Sensitive=拒否。
- Ctx に `driver()` / `audit()` が生え、既存 command は `Driver::Ui` で構築。
- `cargo build` と `cargo test --lib` が緑。既存挙動不変。
- 引き継ぎ課題 2（context.rs のフェーズコメント）が整合。

## 未確定事項（実装計画で詰める）

- Ctx への audit シンク注入方法（`.manage(NoOpAuditSink)` して State 注入 vs
  Ctx が既定で `&NoOpAuditSink` を指す static）。read 系 command は audit を
  発火しないため、4-2 では最小の注入で足りる。
- `AuditEntry` の 4-2 時点フィールド（use_case / risk / driver のみ。timestamp と
  input 概要は 4-4 の SQLite スキーマ確定時。Date/時刻は 4-4 で扱う）。
```
