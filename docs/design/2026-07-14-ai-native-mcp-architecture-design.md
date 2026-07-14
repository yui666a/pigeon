# AI Native アーキテクチャ設計 — MCPサーバー化と常駐エージェント

作成日: 2026-07-14
ステータス: ドラフト（設計合意済み・実装計画は別途）

## 背景と目的

Pigeon は現在、純粋なデスクトップメールクライアントとして開発している。今後、以下2つを実現して **AI Native なアプリケーション**にしたい。

1. **MCP サーバー化** — 外部 LLM から Pigeon のメール／アプリケーション操作（検索・分類・送信・削除・案件移動など）を呼び出せるようにする。
2. **常駐 AI エージェント** — アプリ内に AI エージェントが常駐し、「新着を分類 → 要約 → ドラフト提案」などを自律実行する。

到達目標は最大形（フル操作＝送信・削除含む × 常駐エージェント）。ただし送信・削除のような破壊的・外向き操作を LLM が行うため、**安全・認可の境界**が最重要課題になる。

本設計は 2026-07-13 リファクタリング（PR #102〜#123）後の状態を出発点とする。関連する現状整理は下記アーティファクトのセクション5「正直な現在地」に対応する。

## 設計上の決定（合意事項）

| 論点 | 決定 |
|------|------|
| 操作レベル | フル操作（送信・削除含む）＋ 常駐 AI エージェント（最大形） |
| 安全境界 | 能力ごとの Risk 分類（Read / Reversible / Sensitive）＋ 確認ゲート。アプリケーション層に一元化 |
| 導入順序 | 基盤先行。Phase 4（アプリ層統一＋ゲート＋監査）→ Phase 5（MCP／エージェント driver 追加） |
| 単一入口の形 | Command バス／ユースケースレジストリ（`UseCase` trait: name / risk / run(input, ctx)） |

## 全体アーキテクチャ — 3 driver が 1 つのアプリケーション層を共有する

現状の Pigeon は driver が1つ（React UI → Tauri commands）しかない。目標は同じ use case を **3つの driver** から叩けるようにすること。

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
        ┃  Command バス / dispatch  ┃  ← 単一の chokepoint
        ┃  1. gate.check(risk, ctx) ┃    ・Risk 分類ゲート
        ┃  2. audit.log(...)        ┃    ・監査ログ
        ┃  3. usecase.run(input,ctx)┃    ・実行
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
                     ▼
       UseCase レジストリ（name → trait object）
                     ▼
        既存の port → adapter（DB / IMAP / LLM）
```

### 核心となる3つの考え方

1. **MCP と常駐エージェントは「新しい層」ではなく「新しい driver」**。`commands/` と横並びに位置する。今 `commands/` がやっている「外の要求を use case に翻訳する」仕事を、MCP は「LLM の tool 呼び出しを翻訳」、エージェントは「自分の判断を翻訳」する形で繰り返すだけ。層は増えない。

2. **安全性は use case ごとに1回だけ定義する**。`Risk::{Read, Reversible, Sensitive}` を UseCase 自身が宣言し、dispatch が一元的にゲートをかける。「誰が呼んだか（driver）」は `ctx` に入る。UI の人間クリックは承認済み扱い、外部 LLM の Sensitive は承認待ち、といった差はゲート内の1箇所で決まる。送信・削除のガードが driver ごとに散らばらない。

3. **MCP の tool 定義はレジストリから自動導出する**。各 UseCase が name / 入力スキーマ / risk を持つので、MCP の tool 一覧・JSON Schema・「このtoolは承認が要る」の注記は手書きせずレジストリから生成できる。UI・MCP・エージェントが同じ能力セットを絶対にズレなく共有する。

## 現状分析（実装調査に基づく）

Rust バックエンドの use case / service 層を調査した結果、基盤先行が妥当と確認できた。

### 追い風（すでに正しい形になっている）

- **エラー型はすでに統一済み** — `AppError`（`error.rs`）が全域で使われており、`run(input, ctx) -> Result<Output, AppError>` の error 軸は無改修で揃う。
- **classify / sync / rescan は既に Ctx 非依存** — `classifier/service.rs`、`mail_sync/sync_service.rs`、`project_context/mod.rs` の service 関数は明示的な依存注入＋クロージャで書かれ、Tauri を一切知らない。ほぼそのまま UseCase 化できる。
- **進捗コールバックはすでに `Fn(usize, usize)` の素のクロージャ** — Tauri の `app.emit` は `commands/` 側のラッパ4箇所（`sync-progress` / `backfill-progress` / `classify-progress`）にしかない。driver 非依存化は「クロージャの中身を差し替える」だけで済む。

### 逆風（Phase 4 の主作業）

- **delete / archive / flag / send は command-resident** — 業務ロジックが `commands/` にインラインで存在し、`delete_mail_inner` / `archive_mail_inner`（`mail_commands.rs`）は Tauri `State` を直接受け取っている。`set_flagged` / `mark_unread` / `mark_read` は service 関数すら無い。**これらは同時に最も強くゲートを必要とする Sensitive 操作**であり、Phase 4 の中核はこの抽出になる。
- **共有 `Ctx` が無い** — Tauri の managed State が複数（`DbState` / `SecureStoreState` / `SyncLocks` / `IdleWatchers` / `PendingClassifications` / `ClassifyBatches`）バラバラに注入されている。HTTP クライアントや LLM 設定は State に無く、`build_classifier` がリクエストごとに settings テーブルと SecureStore から構築している。
- **`UseCase` / risk / gate 抽象は皆無** — 認可層は存在しない。既存のポリシーは2つの純関数モジュールのみ（下記）。

### 既存ポリシーの位置づけ（消さずに残す）

- **`commands/mail_policy.rs`** — サーバー反映の要否判定（`plan_delete` / `plan_archive` / `is_local_only_folder`）。「どう実行するか」のドメインポリシー。
- **`project_context/cloud_policy.rs`** — クラウド送信可否のフェイルクローズ判定（`is_cloud_allowed`）。ファイル由来データのクラウド LLM 送信フィルタ。

新設する Risk ゲートは「実行してよいか（誰が）」の**認可**ポリシーで、上記2つの**実行方法**ポリシーとは層が違う。両者は補完関係であり、Risk ゲート導入時に既存ポリシーは変更しない。

## コンポーネント設計

### Ctx（共有コンテキスト）

現状バラバラな managed State を束ねた依存アクセサ。dispatch が driver ごとに構築して渡す。

- DB アクセス（`with_conn` / `with_conn_mut` 相当）
- SecureStore
- 揮発状態（`PendingClassifications` / `ClassifyBatches` / `SyncLocks`）
- **driver 情報**（`Driver::{Ui, Mcp{...}, Agent{trust}}`）— ゲートの判定材料
- **通知シンク**（`ctx.progress(done, total)` / `ctx.notify(event)`）— driver ごとに実体を差し替える

### UseCase trait とレジストリ

```
trait UseCase {
    fn name(&self) -> &str;
    fn risk(&self, input: &Input) -> Risk;   // input 依存（archive のプラン依存 Risk 等）
    fn run(&self, input: Input, ctx: &Ctx) -> Result<Output, AppError>;
}
```

- 各 use case を name / 入力スキーマ / risk を持つ実装として登録。
- `risk()` は input を受け取る。多くの use case は input を無視して固定 Risk を返すが、archive のようにプランで Risk が変わるものは input から実効プランを引く。
- レジストリ = name → trait object のマップ。MCP の tool 一覧・JSON Schema はここから導出。
- sync / async の非一様性は、trait 側を async に寄せる（sync な use case は即時 return）ことで吸収する。

### dispatch（Command バス）

```
dispatch(name, input, ctx):
    uc   = registry.lookup(name)
    risk = uc.risk(&input)                // input 依存の実効 Risk
    gate.check(risk, ctx.driver)          // 認可
    audit.log(name, risk, ctx.driver, input概要)
    result = uc.run(input, ctx)           // 実行
    audit.log_result(...)
    return result
```

3 driver すべてがこの1関数を通る。特権的な裏口は設けない（エージェントもゲートを迂回できない）。

### Risk ゲートと承認キュー

- `Risk::Read` — 自由に実行。
- `Risk::Reversible` — 自動実行＋監査ログ（例: フラグ、未読戻し、案件移動）。
- `Risk::Sensitive` — 人間の承認必須（例: 送信、サーバー削除）。承認キューに積んで停止。

Risk は use case 単位で固定せず、実行計画に応じて決まる場合がある点に注意する。特に **archive は Gmail 等で `plan_archive` が `CopyThenDelete`（サーバー削除を伴う）に解決されるため Sensitive、ローカルのみで完結する場合は Reversible** になる。UseCase の `risk()` は入力から実効プランを引いて Risk を返す（＝`risk()` は input を参照できる）。この「プラン依存 Risk」の代表例が archive であり、mail_policy の判定結果と Risk 分類はここで接続する。
- **承認キューは3 driver 共通の1つ**。UI が人間クリックで承認、エージェント／MCP がそこに積む。「保留中の Sensitive 操作」がアプリに1つのリストとして見える（UI 側の新機能）。
- driver によるゲート分岐: 内蔵エージェントの Sensitive は承認キュー経由、外部 MCP の Sensitive は拒否またはより厳しい承認、といった差はゲート内で `ctx.driver` を見て決める。

### 進捗・通知の逆流路（driver 非依存）

use case が外へ通知する経路は、既存の `Fn(usize, usize)` クロージャをそのまま活かす。`Ctx` の通知シンクが driver ごとに実体を持つ。

| driver | 通知シンクの実体 |
|--------|-----------------|
| commands（UI） | `app.emit("sync-progress", …)` ← 現状のまま |
| MCP | MCP の progress notification（または stream で途中経過） |
| 常駐エージェント | エージェント内部状態を更新（emit 不要） |

classify / sync / backfill はシグネチャ変更なしで bus に載る。

### 常駐 AI エージェント（driver の1つ）

```
    trigger（new-mail / ユーザー指示 / スケジュール）
         │
         ▼
    Agent ループ:
      1. LLM に「使える tool 一覧」を渡す   ← レジストリから導出（UI/MCPと同一）
      2. LLM が tool を選ぶ
      3. dispatch(name, input, ctx)          ← 同じ chokepoint
      4. Sensitive なら承認キューに積んで停止 ← 人間の承認待ち
      5. 結果を LLM に返して継続
```

- エージェントの LLM 呼び出しは既存の `classifier` の LLM 抽象（`TextGenerator` 等）を再利用。新しい LLM クライアントは作らない。tool-calling 対応の薄い拡張で足りる。
- エージェントの `ctx.driver = Agent{trust: high}`。ゲートの判定材料になる。

## テスト戦略

- **UseCase 単体**: Ctx をモック（インメモリ DB／スタブ classifier）で注入し、run を検証。既存の port モック戦略を踏襲。
- **dispatch／ゲート**: driver × risk の組み合わせで、Read=通過 / Reversible=通過+監査 / Sensitive=承認キュー投入 を検証。
- **抽出リグレッション**: send / delete / archive / flag を service へ引き剥がす際、抽出前後で挙動不変であることを既存の command 経由テストで担保（TDD: 先に現挙動を固定するテストを書く）。
- **アダプタ**: 実 SQLite で DB アダプタ、実 IMAP/SMTP は既存の統合テスト方針に従う。

## フェーズ分けと PR 粒度

各行が概ね1 PR（Single Concern）。

### Phase 4: 基盤（MCP コードは1行も書かない）

| # | 内容 |
|---|------|
| 4-1 | **Ctx 導入** — 散らばった State を束ねる依存アクセサ。既存 command をまず Ctx 経由に付け替えるだけ（挙動不変・純リファクタ） |
| 4-2 | **UseCase trait + レジストリ + dispatch の骨格** — まず read 系（search / list）だけ載せる。`Risk::Read` のみ |
| 4-3 | **Sensitive 抽出** — send / delete / archive / flag を command から Ctx 非依存な usecase へ引き剥がす |
| 4-4 | **Risk ゲート + 監査ログ + 承認キュー**（バックエンド側） |
| 4-5 | **既存 classify / sync / rescan を bus に載せ替え**（ほぼ無改修） |

### Phase 5: driver 追加

| # | 内容 |
|---|------|
| 5-1 | **MCP server driver** — レジストリから tool 定義を自動導出。外部 LLM 接続 |
| 5-2 | **承認キューの UI** — 保留 Sensitive の一覧・承認（フロントのストア整理と接続） |
| 5-3 | **常駐エージェント driver** |

### 依存関係

```
4-1 → 4-2 → {4-3, 4-5} → 4-4 → Phase 5
```

4-3 と 4-5 は並行可能（ただし開発運用メモの「並行実装は最大3体」制約に従う）。

**4-1〜4-3 は MCP と無関係に「今のリファクタリングの続き」として単独で価値がある**（section 5 の「port 水平展開」と同じ精神で「use case を command から水平に引き剥がす」）。Phase 5 に進まなくてもバックエンドは健全になる。これが foundation-first の本質。

## フロントエンドとの接続への波及

section 5 で指摘済みの「フロントのストアがアプリケーション層を兼務」問題は、承認キュー UI（5-2）と接続する。保留 Sensitive 操作の一覧という新しい共有状態が入るため、mailStore の分割・上位フックへのオーケストレーション抽出はこのタイミングで整理する。

## スコープ外（YAGNI）

- 外部 LLM のマルチテナント・複数ユーザー認可（個人開発アプリのため単一ユーザー前提）。
- エージェントの高度なプランニング／マルチステップ自律度の作り込みは Phase 5-3 で最小構成から始める。
- 既存2ポリシー（mail_policy / cloud_policy）の再設計は行わない。

## 未確定事項（実装計画で詰める）

- `UseCase` の Input/Output をどう型付けするか（enum ディスパッチ vs ジェネリクス vs serde_json::Value 経由）。実コードの service シグネチャを見て 4-2 で確定。
- 監査ログの保存先（settings テーブル同様の SQLite テーブル vs ファイル）。
- 承認キューの永続化要否（アプリ再起動で保留操作を残すか）。
