# ADR 0004: AI-Native dispatch バスと Risk 認可アーキテクチャ

## ステータス

進行中（2026-07-14 起草、2026-07-15 更新）。Phase 4-1（Ctx 導入）〜 4-4（ゲート本体 + 監査永続化 + 承認キュー）まで実装済み。4-5 のうち async UseCase 対応は 4-3 の Sensitive 抽出（IMAP/SMTP を伴う）に必要になったため前倒しで導入済み。残りは classify / sync / rescan の bus 載せ替え（4-5）と Phase 5（driver 追加・承認 UI）。

実装時に確定した設計差分（下記の各決定に追記済み）:

- `UseCase::risk(&self, input, ctx) -> Result<Risk, AppError>` — プラン依存 Risk（archive/delete）は mail_policy の実効プランを DB から引くため ctx が要る
- `gate::check(risk, driver) -> GateOutcome { Allow, RequireApproval }` — キュー投入は use case 名と input を持つ dispatch 側が行う
- `AuditSink::record(&self, conn, &AuditEntry)` — SQLite シンクが接続を借りる形。監査は fail-open（書き込み失敗で操作を止めない。警告ログ）
- 監査は実行「前」に記録する（実行失敗も試行として残る）。承認キューへ積んだ操作は audit_log ではなくキュー自体が記録

このドキュメントは、以下の設計群に分散していた決定を 1 本の集約版として恒久保持するために作成する。

- `docs/design/2026-07-14-ai-native-mcp-architecture-design.md`（中核設計）
- `docs/design/2026-07-14-phase4-2-usecase-bus-design.md`（Phase 4-2 詳細設計）
- リファクタリング調査レポート（`Report.md`、2026-07-13）の §2.4 / §2.5

## コンテキスト

### Pigeon を AI-Native にする目標

Pigeon は現在、純粋なデスクトップメールクライアントとして開発している。今後、以下 2 つを実現して AI-Native なアプリケーションにしたい。

1. MCP サーバー化。外部 LLM から Pigeon のメール操作（検索・分類・送信・削除・案件移動など）を tool として呼び出せるようにする。
2. 常駐 AI エージェント。アプリ内に AI エージェントが常駐し、「新着を分類 → 要約 → ドラフト提案」などを自律実行する。

到達目標は最大形である。すなわちフル操作（送信・削除を含む）× 常駐エージェントであり、破壊的・外向き操作を LLM が行う。したがって安全・認可の境界が最重要課題になる。

### 既存のコマンド直呼び構造の限界

出発点は 2026-07-13 リファクタリング（PR #102〜#123）後の状態である。Report.md の診断が示すとおり、現状の Pigeon には driver が 1 つ（React UI → Tauri commands）しかなく、以下の構造的負債を抱えている。

- ユースケース層が存在しない。`commands/` と Zustand ストアが業務ロジックを兼務しており（Report.md §2.1）、これが commands 層テスト 0 件の根本原因になっている。
- 破壊的操作（`delete` / `archive` / `flag` / `send`）が command-resident である。`delete_mail_inner` / `archive_mail_inner` は Tauri `State` を直接受け取り、`set_flagged` / `mark_unread` / `mark_read` は service 関数すら存在しない。これらは同時に最も強くゲートを必要とする Sensitive 操作である。
- 認可層（`UseCase` / risk / gate 抽象）が皆無である。

MCP と常駐エージェントを追加すると、送信・削除のガードが driver ごとに散らばる。外部 LLM 用に書いたガードと UI 用のガードがズレれば、破壊的操作が抜け道から漏れる。Risk 認可と監査を driver 横断で一箇所に効かせたい、というのが本アーキテクチャの動機である。

### あるべき姿は既にコードベース内にある

Report.md §1・§9 が指摘するとおり、`classifier` の trait port（`LlmClassifier` / `TextGenerator`）と `project_context::rescan_project`（`&dyn TextGenerator` を注入で受けるユースケース関数）が、依存注入とテスト容易性の好例として既に存在する。本アーキテクチャは新パラダイムの導入ではなく、この既存パターンを「use case を command から水平に引き剥がす」形で展開するものである。

## 決定

### D1. dispatch バス中心設計 — 3 driver が 1 つのアプリケーション層を共有する

MCP と常駐エージェントは「新しい層」ではなく「新しい driver」として位置づける。`commands/`（人間 / UI）と横並びに、MCP server（外部 LLM）と resident agent（アプリ内ループ）を置く。3 つの driver はすべて単一の dispatch バス（Command バス）という chokepoint を通る。

```
        driver 層（外向き・入口）
   ┌──────────┬──────────────┬─────────────────┐
   │ commands │  MCP server  │ resident agent  │
   │ (人間/UI)│ (外部LLM)    │ (アプリ内ループ) │
   └────┬─────┴──────┬───────┴────────┬─────────┘
        │            │                │
        └────────────┼────────────────┘
                     ▼
        ┏━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃  dispatch（Command バス）  ┃  ← 単一の chokepoint
        ┃  1. lookup(name)          ┃
        ┃  2. risk = uc.risk(input) ┃
        ┃  3. gate.check(risk, ctx) ┃  ・Risk 認可ゲート
        ┃  4. audit.record(...)     ┃  ・監査ログ
        ┃  5. uc.run(input, ctx)    ┃  ・実行
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
                     ▼
       UseCase レジストリ（name → trait object）
                     ▼
        既存の port → adapter（DB / IMAP / LLM）
```

エージェントを含むいかなる driver も、特権的な裏口（ゲートを迂回する経路）を持たない。

### D2. UseCase trait の 2 層構造（型安全 trait + ErasedUseCase 消去層）

UseCase を 2 層の trait で表現する。

- 上層 `UseCase`（実装者が書く・型安全・同期）。関連型 `type Input: DeserializeOwned` / `type Output: Serialize` を持ち、`name()` / `risk(&Input) -> Risk` / `run(Input, &Ctx) -> Result<Output, AppError>` を宣言する。
- 下層 `ErasedUseCase`（消去層・`dyn` 可・ブランケット実装で自動導出）。`name()` / `risk_json(&Value)` / `run_json(Value, &Ctx) -> Result<Value, AppError>` を持ち、`impl<T: UseCase> ErasedUseCase for T` で自動導出する。

実装者は型安全な `UseCase` だけを書く。レジストリ登録・MCP tool 自動導出・3 driver 共有は消去層が担保する。

### D3. Risk による認可ゲート — `Risk::{Read, Reversible, Sensitive}`

各 UseCase が自身の Risk を宣言し、dispatch が一元的にゲートをかける。

- `Risk::Read`。自由に実行（検索・一覧）。
- `Risk::Reversible`。自動実行 + 監査ログ（フラグ、未読戻し、案件移動）。
- `Risk::Sensitive`。人間の承認必須（送信、サーバー削除）。UI driver は人間のクリック自体が承認とみなして実行し、MCP / Agent driver は承認キューに積んで停止する（4-4 実装済み）。

Risk は use case 単位で固定せず、実行計画に応じて決まる場合がある。`risk()` は input を参照できる。代表例は archive で、Gmail 等では `plan_archive` が `CopyThenDelete`（サーバー削除を伴う）に解決されるため Sensitive、ローカルのみで完結する場合は Reversible になる。この「プラン依存 Risk」で mail_policy の判定結果と Risk 分類が接続する。

### D4. Driver enum — `Driver::{Ui, Mcp, Agent}`

「誰が呼んだか」を `Ctx` の `driver: Driver` に持たせ、ゲートの判定材料とする。UI の人間クリックは承認済み扱い、外部 LLM の Sensitive は拒否またはより厳しい承認、常駐エージェントの Sensitive は承認キュー経由、といった差はゲート内の 1 箇所で `ctx.driver()` を見て決める。enum は 3 バリアントで最小とし、分岐の中身は Phase 5 で実装する。

### D5. dispatch パイプライン（lookup → risk → gate → audit → run）

dispatch は次の固定パイプラインを取る。

```
dispatch(registry, name, input, ctx):
    uc   = registry.lookup(name)          // 未登録は AppError::Validation
    risk = uc.risk_json(&input)           // input 依存の実効 Risk
    gate::check(risk, ctx.driver())       // 認可
    if risk != Read: ctx.audit().record(AuditEntry::new(name, risk, driver))
    return uc.run_json(input, ctx)        // 実行
```

Phase 4-2 では Read のみが載るため gate/audit は実質未発火だが、パイプラインの形をここで確定させ、以降のフェーズが拡張点を増やさずに差し込めるようにする。

### D6. run は同期で確定し、async は後回しにする（YAGNI）

Phase 4-2 で載せる read 系は全て同期である。`run` は同期で確定する。async 化（`async_trait` または `run` の async 化）は、実際に async な UseCase（classify / sync）を載せる Phase 4-5 で導入する。載せる use case が実在しない段階で async を先決めしない。

### D7. 新しいエラー型を足さず `AppError::Validation` に載せる（YAGNI）

JSON パース失敗、未登録 name、Reversible / Sensitive のゲート拒否（4-4 で置換）はすべて既存の `AppError::Validation` に載せる。`AppError` は全域で統一済みであり、`run(input, ctx) -> Result<_, AppError>` の error 軸は無改修で揃う。新バリアントは追加しない。

### D8. Driver enum を先行確定して拡張点を固定する

Phase 4-2 の呼び出し元は UI（commands）のみだが、`Driver::{Ui, Mcp, Agent}` を先に確定し、`gate::check(risk, ctx.driver())` のシグネチャを 4-2 で固定する。これにより Phase 5 で MCP / Agent driver を差し込むとき、ゲート引数を増やす破壊的変更が発生しない。既存 `Ctx::new` は `Driver::Ui` 固定で構築し、`new_for_test` も `Ui` 既定とする。

### D9. 監査テーブル・承認キュー・MCP コードを先行導入しない（foundation-first）

Phase 4 では MCP コードを 1 行も書かない。`AuditSink` trait は用意するが、Phase 4-2 の実体は `NoOpAuditSink` / `InMemoryAuditSink` のみとする。監査ログの SQLite テーブル（migration）と `SqliteAuditSink`、承認キュー本体（テーブル・投入ロジック）は、実際に Reversible / Sensitive を載せる Phase 4-4 で導入する。使われないテーブルを先行導入しない。

### D10. 既存ポリシーは変更しない — Risk ゲートと層が違う

新設する Risk ゲートは既存の 2 ポリシーとは別の層に属する。3 者は補完関係であり、Risk ゲート導入時に既存ポリシーは変更しない（詳細は下の「Risk ゲートと既存ポリシーの層の違い」を参照）。

### D11. フェーズ分けと依存関係

各行が概ね 1 PR（Single Concern）である。

Phase 4（基盤・MCP コードなし）。

| # | 内容 |
|---|------|
| 4-1 | Ctx 導入（散らばった State を束ねる依存アクセサ。挙動不変の純リファクタ）— 完了済み |
| 4-2 | UseCase trait + レジストリ + dispatch の骨格。read 系のみ `Risk::Read` で載せる |
| 4-3 | Sensitive 抽出（send / delete / archive / flag を command から Ctx 非依存な usecase へ引き剥がす）— 完了（2026-07-15）。async UseCase 対応もここで前倒し導入 |
| 4-4 | Risk ゲート本体 + 監査ログ SQLite 永続化（audit_log, v15）+ 承認キュー（approval_queue, v16）— 完了（2026-07-15）。承認キューの消費 UI と承認後の再実行は 5-2 |
| 4-5 | 既存 classify / sync / rescan を bus に載せ替え（ほぼ無改修） |

Phase 5（driver 追加）。

| # | 内容 |
|---|------|
| 5-1 | MCP server driver（レジストリから tool 定義を自動導出。外部 LLM 接続） |
| 5-2 | 承認キューの UI（保留 Sensitive の一覧・承認） |
| 5-3 | 常駐エージェント driver |

依存関係。

```
4-1 → 4-2 → {4-3, 4-5} → 4-4 → Phase 5
```

4-3 と 4-5 は並行可能（ただし開発運用メモの「並行実装は最大 3 体」制約に従う）。4-1〜4-3 は MCP と無関係に「今のリファクタリングの続き」として単独で価値がある。Phase 5 に進まなくてもバックエンドは健全になる。これが foundation-first の本質である。

### D12. 単件版と一括版が重複する use case は一括版に一本化する

同じ操作に対して「単件版」と「一括版」の use case を両方定義しない。一括版が `Vec<Id>` を取るなら、要素 1 個の呼び出しが単件操作を完全に代替するためである。

この判断により `MoveMailUseCase` を廃止し、`BulkMoveMailsUseCase` に一本化した（単件移動は `mail_ids` に 1 要素を渡す）。本体の `move_mail_inner` は一括版から 1 件ずつ再利用されるため存続する。

理由は D1（単一 dispatch バス）および却下案 A1 と同じである。同一の操作ロジックに入口が 2 つあると、Risk 宣言・入力スキーマ・監査名が二重メンテになり、両者がズレたときに片方だけがゲートを迂回する余地が生まれる。MCP tool は各 use case から自動導出されるため、重複はそのまま外部 LLM に見える tool の重複になる。

一括版のみが `BulkResult`（部分失敗の積み上げ）を返すため、単件呼び出しでも戻り値は `BulkResult` になる。要素 1 個ぶんの冗長さは、二重メンテのコストに見合わないと判断した。

将来 UI・MCP のいずれかが「単件専用の軽い戻り値」を必要とする場合も、新しい use case を足すのではなく一括版の戻り値設計を見直す。

### なぜ dispatch バス中心か（D1 の根拠）

安全性を use case ごとに 1 回だけ定義するためである。Risk と入力スキーマを UseCase 自身が宣言し、dispatch が一元的にゲートをかければ、送信・削除のガードが driver ごとに散らばらない。さらに MCP の tool 一覧・JSON Schema・「この tool は承認が要る」の注記は、各 UseCase が持つ name / 入力スキーマ / risk からレジストリ経由で自動導出できる。UI・MCP・エージェントが同じ能力セットを絶対にズレなく共有する。

### なぜ 2 層 trait か（D2 の根拠）

ジェネリクス trait は関連型（`type Input`）を持つため `dyn` 化できない。関連型が呼び出し側から見えないからである。一方、3 driver が name でレジストリを引くには「型を消した共通の入口」が要る。この 2 つの要求（実装者は型安全に書きたい / レジストリは型を消したい）を 2 層 trait で両立する。ブランケット実装 `impl<T: UseCase> ErasedUseCase for T` により、実装者が型付き `UseCase` を書くだけで消去層が自動的に付いてくる。`risk_json` が input を deserialize するのは、`risk()` が input 依存（プラン依存 Risk）だからである。

### なぜ同期 run か（D6 の根拠）

4-2 で載る read 系は全て同期であり、async な use case は 4-2 の段階に実在しない。実在しない要求のために `async_trait` の複雑さを先に払うのは YAGNI である。async は classify / sync を載せる 4-5 で、実物を見て導入する方が設計判断を誤らない。

### なぜ Risk ゲートと既存ポリシーを分けるのか（D10 の根拠）— 層の違い

Pigeon には Risk ゲート導入前から 2 つの純関数ポリシーモジュールがあり、これらは Risk ゲートとは層が異なる。

| ポリシー | 場所 | 問い | 層 |
|----------|------|------|----|
| `mail_policy` | `src-tauri/src/commands/mail_policy.rs` | どう実行するか（`plan_delete` / `plan_archive` / `is_local_only_folder`。サーバー反映の要否） | 実行方法（ドメインポリシー） |
| `cloud_policy` | `src-tauri/src/project_context/cloud_policy.rs` | 送ってよいか（`is_cloud_allowed`。ファイル由来データのクラウド LLM 送信可否のフェイルクローズ判定） | データ送信可否 |
| Risk ゲート（新設） | `usecase/gate.rs` | 実行してよいか・誰が（driver × Risk の認可） | 認可 |

`mail_policy` は「どう実行するか」、`cloud_policy` は「（データを）送ってよいか」を決める。Risk ゲートは「（操作を）実行してよいか、誰が」を決める。3 者は関心が直交しており、補完関係にある。特に archive は、mail_policy の `plan_archive` が返す実効プラン（`CopyThenDelete` か否か）を Risk ゲートが読んで Sensitive / Reversible を決めるという形で接続する。認可（Risk）とデータ送信可否（cloud_policy）と実行方法（mail_policy）を混ぜないことで、それぞれを独立に変更・テストできる。したがって Risk ゲート導入時に既存 2 ポリシーは一切変更しない。

## 却下した代替案

### A1. driver ごとに use case を別実装する

MCP 用・エージェント用・UI 用に操作ロジックを個別に書く案。送信・削除のガードが driver ごとに散らばり、外部 LLM 用ガードと UI 用ガードがズレたときに破壊的操作が抜け道から漏れる。能力セットの三重メンテも発生する。単一 dispatch バス（D1）で棄却。

### A2. Risk 認可を driver 層（各 command / 各 tool ハンドラ）に置く

ゲートを入口ごとに書く案。chokepoint が消え、「特権的な裏口を作らない」保証が失われる。エージェントがゲートを迂回できてしまう。dispatch を単一 chokepoint とする D1・D5 で棄却。

### A3. 最初から async trait で `run` を定義する

将来の classify / sync を見越して 4-2 から `async_trait` を導入する案。載せる async use case が実在しない段階での先決めであり YAGNI。同期 run（D6）で棄却し、async は 4-5 に送る。

### A4. UseCase 用の新しいエラー型を追加する

パースエラー・ゲート拒否・未登録 name のために `AppError` へ新バリアントを足す案。既存 `AppError::Validation` で十分表現でき、error 軸は無改修で揃う。新バリアントは維持コストに見合わない。`AppError::Validation` への集約（D7）で棄却。

### A5. 監査テーブル・承認キューを先行導入する

Phase 4-2 の段階で SQLite の監査テーブル（migration）と承認キューを作る案。4-2 で載るのは Read のみで、これらは使われない。使われないテーブルの先行導入は YAGNI。`NoOpAuditSink` / `InMemoryAuditSink` に留め、実体は Reversible / Sensitive を載せる 4-4 で入れる（D9）で棄却。

### A6. Driver 情報を dispatch の引数で都度渡す

`Ctx` に driver を持たせず、dispatch 呼び出しごとに driver を明示的に渡す案。ゲートのシグネチャが不安定になり、driver を増やすたびに呼び出し側の変更が波及する。`Ctx::driver()` として束ね、enum を先行確定する（D4・D8）で棄却。

## 影響

### Phase 4-2 以降の実装指針

- 新規モジュールは `src-tauri/src/usecase/` に置く（commands と横並びのアプリケーション層）。構成は `mod.rs` / `risk.rs` / `driver.rs` / `traits.rs`（UseCase + ErasedUseCase + ブランケット実装）/ `registry.rs` / `gate.rs` / `audit.rs` / `dispatch.rs` / `cases/`。
- `src-tauri/src/context.rs`（Phase 4-1 で導入済み）に `driver: Driver` フィールドと audit シンク参照を追加し、`driver()` / `audit()` アクセサを生やす。既存 command は `Driver::Ui` で構築する。context.rs の「Risk ゲート等は Phase 4-4 で載せる」旨のフェーズコメントは、ゲート骨格を導入する 4-2 に合わせて修正する。
- `src-tauri/src/lib.rs` に `pub mod usecase;` を追加し、audit シンク（`NoOpAuditSink`）を注入する。

### 新 UseCase を追加する手順

1. 型安全な `UseCase`（`Input` / `Output` / `name` / `risk` / `run`）を実装する。破壊的操作は `run` を Ctx 非依存な service 関数に委譲する（4-3 の Sensitive 抽出と同じ形）。
2. `risk()` で Risk を宣言する。プラン依存なら input から実効プランを引く。
3. レジストリに `register` する。
4. これだけで dispatch を通り、ゲート・監査が自動で効き、MCP tool としても自動導出される（Phase 5-1）。driver ごとの追加実装は不要。

### 拡張点（破壊的変更を起こさない固定点）

- `gate::check(risk, driver) -> GateOutcome`（4-4 で確定）。ゲートは判定のみを返し、承認キュー投入は use case 名と input を持つ dispatch が行う。4-2 時点の `Result<(), AppError>` 版から変更した。
- `Driver` enum は 3 バリアントで固定。Phase 5 で分岐の中身のみ実装する。
- `AuditSink` trait は 4-2 で固定。4-4 で `SqliteAuditSink` を差し込む。`AuditEntry` のフィールド（4-2 は use_case / risk / driver のみ）は 4-4 の SQLite スキーマ確定時に timestamp・input 概要を足す。
- 承認キューは 3 driver 共通の 1 つとする。UI に「保留中の Sensitive 操作」の一覧という新しい共有状態が入る（Phase 5-2）。これに合わせてフロントの mailStore 分割・上位フックへのオーケストレーション抽出を整理する（Report.md §4.5・§2.1 で指摘済みの「フロントのストアがアプリケーション層を兼務」問題の解消点）。

### スコープ外（YAGNI）

- 外部 LLM のマルチテナント・複数ユーザー認可（個人開発アプリのため単一ユーザー前提）。
- エージェントの高度なプランニング・マルチステップ自律度の作り込み（Phase 5-3 で最小構成から）。
- 既存 2 ポリシー（mail_policy / cloud_policy）の再設計。

### ドキュメント運用方針

- 本 ADR（0004）と中核設計 `2026-07-14-ai-native-mcp-architecture-design.md` は恒久保持する。アーキテクチャの決定と全体像を残す文書だからである。
- Phase 4-2 のような各 Phase の実装詳細スペック（`2026-07-14-phase4-2-usecase-bus-design.md` など）は、当該 Phase の実装完了後に archive へ移す。実装が済めばコードが正であり、詳細スペックは履歴的価値のみになるためである。archive 後も、決定の要旨は本 ADR に集約されている状態を保つ。

## 参照

- `docs/design/2026-07-14-ai-native-mcp-architecture-design.md` — AI-Native アーキテクチャ中核設計（現役 design、恒久保持）。
- `docs/design/2026-07-14-phase4-2-usecase-bus-design.md` — Phase 4-2 UseCase バス詳細設計（現役。4-2 実装完了後に archive）。
- `Report.md`（リファクタリング調査レポート、2026-07-13）— §2.4 境界づけられたコンテキスト / §2.5 目標ディレクトリ構成 / §1 アーキテクチャの核心。本アーキテクチャの原資となった診断。
- `docs/design/2026-04-12-pigeon-design.md` — Pigeon 全体設計書。
- `src-tauri/src/commands/mail_policy.rs` / `src-tauri/src/project_context/cloud_policy.rs` — Risk ゲートと層の違う既存ポリシー（変更しない）。
- `src-tauri/src/context.rs` — Phase 4-1 で導入済みの `Ctx`。4-2 で driver / audit を追加する。
