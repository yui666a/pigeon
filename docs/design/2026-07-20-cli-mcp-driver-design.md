# CLI / MCP driver 設計

- 日付: 2026-07-20
- 関連 ADR: [0004 AI-Native dispatch アーキテクチャ](../adr/0004-ai-native-dispatch-architecture.md)
- 関連設計: [2026-07-14 AI-Native MCP アーキテクチャ](2026-07-14-ai-native-mcp-architecture-design.md), [2026-07-14 Phase 4-2 UseCase バス](../superpowers/specs/2026-07-14-phase4-2-usecase-bus-design.md)
- BACKLOG: 4-5（一部）, 5-1

## 背景と目的

Pigeon の操作は現在 GUI からしか行えない。アプリ画面でできることを CLI と MCP からも実行できるようにする。

ADR 0004 はこれを最初から見越しており、`usecase/dispatch.rs` が「全 driver が通る単一の chokepoint」として実装済みで、`Driver` enum には `Ui` / `Mcp` / `Agent` が、gate マトリクスには Mcp/Agent の Risk ポリシーが既に存在する。本設計はその Phase 5-1 を実行し、あわせて CLI を 4 本目の driver として追加する。

CLI を MCP と別に用意するのは、コンテキスト効率のためである。MCP は全 tool の JSON Schema が常時エージェントのコンテキストに載るが、CLI は `--help` を一度読むだけで済む。両者は競合せず、同じ dispatch バスの上に載る 2 つの薄いプロトコル変換層として共存する。

## アーキテクチャ

```
pigeon (GUI, Tauri)  ─┐
pigeon-cli <verb>    ─┼─→ Ctx{driver} → dispatch() → gate → audit → UseCase
pigeon-cli mcp       ─┘
```

`dispatch` から下は一切変更しない。新規 driver は引数を `serde_json::Value` に変換して `dispatch` に渡し、戻り値を整形するだけの層に留める。ADR 0004 の「特権的な裏口を作らない」という原則を維持する。

## 成果物

`src-tauri/Cargo.toml` に `[[bin]] pigeon-cli` を 1 つ追加する。`[lib]` の `crate-type` に `rlib` が含まれているため `pigeon_lib` を参照できる。

MCP サーバーは独立バイナリにせず、`pigeon-cli mcp` サブコマンドで stdio サーバーとして起動する。MCP クライアント側の設定は次のようになる。

```json
{ "command": "pigeon-cli", "args": ["mcp"] }
```

CLI と MCP を同一バイナリに同居させることで、両者の差分が driver の指定と入出力形式だけであることがコード上で明示される。GUI バイナリとは分離し、CLI 起動が Tauri のウィンドウ初期化に引きずられないようにする。

## Driver と Risk ポリシー

`Driver` enum に `Cli` を追加する。CLI は起動環境によって人間の操作かエージェントの操作かが変わるため、単一の Risk ポリシーでは表現できない。

| Risk | Ui | Cli (TTY) | Cli (非TTY) | Mcp | Agent |
|---|---|---|---|---|---|
| Read | 通過 | 通過 | 通過 | 通過 | 通過 |
| Reversible | 通過 | 通過 | 通過 | 通過 | 通過 |
| Sensitive | 即時許可 | 即時許可 | 承認キュー | 承認キュー | 承認キュー |

判定には **stdin の TTY 有無**を用いる。stdout で判定すると、利用者が `pigeon-cli ... | tee log` のように出力をパイプしたときに人間の操作が非 TTY と誤判定される。stdin は出力をパイプしても端末のまま残るため、判定軸として適切である。

この方式が機能することは検証済みで、Claude Code の Bash ツールは stdin/stdout/stderr のいずれにも TTY を割り当てない。したがってエージェント経由の起動は正しく非 TTY と判定され、Sensitive 操作は承認キューに積まれる。

`--yes` のようなフラグによる自己申告方式は採らない。フラグはエージェントも打てるため、gate を迂回する裏口になる。起動環境による判定は、意思表示ではなく事実に基づくため迂回できない。

`Ctx::with_driver()` は現在 `#[cfg(test)]` 限定であり、本番コードから非 UI driver を構築できない。この属性を解除する。

## CLI インターフェース

発見性と網羅性を両立するため 2 層構成とする。

**名前付きサブコマンド**は頻用の操作に用意する。`sync`, `search`, `projects`, `threads` など。人間が読んで理解でき、`--help` に列挙される。

**汎用ディスパッチ** `pigeon-cli call <usecase-name> '<json>'` を常に併設する。バスに載った UseCase はサブコマンドを書かずとも即座に叩けるため、サブコマンド定義の遅れが機能の遅れにならない。

出力はデフォルトを人間向けテキスト、`--json` で機械可読な JSON とする。エラーは stderr に出力し、終了コードを非ゼロとする。

## Registry の拡張

現在の `Registry` は `lookup` のみを公開しており、内部の `map` が private のため登録済み UseCase を列挙できない。MCP の tool list も CLI の `call` の名前解決も作れないため、以下を追加する。

- 登録済み UseCase の列挙 API（名前とメタデータ）
- 各 UseCase の JSON Schema 導出

Schema は MCP の tool definition と CLI の引数検証の双方で共用する。

## UseCase の載せ替え範囲

CLI/MCP から呼べるのはバスに載った UseCase だけである。現在 18 個が登録済みだが、Tauri command は 70 個あり、大半が未載せ替えである。今回は全面載せ替えを行わず、エージェントの主要導線を閉じるために必要な範囲に限定する。

**書き込み系（BACKLOG 4-5 の一部）**

- `sync_account`
- `classify_batch`

これにより「同期 → 分類 → 検索 → 返信」が CLI/MCP から完結する。`sync_account` は `SyncLocks` による多重起動ガードが現在 command 関数内にあるため、UseCase 側へ移す。

この 2 つは進捗シンク（次節）の追加を前提とする。read 系 4 つは純粋な `with_conn` 呼び出しであり、既存の `SearchMailsUseCase` と同じ形に 1:1 で移せる。

## 進捗シンク

`sync_account` と `classify_batch` は現在 `AppHandle` を使って進捗イベントを emit している。

- `sync_account` → `app.emit("sync-progress", ...)`
- `classify_batch` → `app.emit("classify-progress", ...)`

`Ctx` は `AppHandle` を持たないため、このままではバスに載らない。`Ctx` に進捗シンクを追加する。

```rust
audit: Option<&'a dyn AuditSink>,
progress: Option<&'a dyn ProgressSink>,   // 追加
```

進捗の出し方は driver ごとに異なるのが自然であり、「全 driver が共有する借用コンテキスト」という `Ctx` の役割に合致する。既存の `AuditSink` と同じ形（`Option<&dyn Trait>` + 既定実装へのフォールバック）に揃える。

| Driver | 実装 |
|---|---|
| Ui | `app.emit(...)` で既存のイベント名をそのまま発行 |
| Cli | stderr に進捗表示（stdout は結果専用に保つ） |
| Mcp | 破棄（no-op） |

`AuditSink` と同様、進捗の送出失敗は本処理を止めない（ベストエフォート）。既存コードも `let _ = app.emit(...)` で失敗を無視しており、その挙動を維持する。

### `spawn_embedding_pass` の扱い

`sync_account` は同期成功後に `spawn_embedding_pass(app)` で埋め込み生成タスクを起動している。これは進捗通知とは別種の `AppHandle` 依存であり、進捗シンクでは解決しない。

これは UseCase の外に残し、**Tauri command 側で dispatch 成功後に呼ぶ**。CLI から同期した場合は埋め込み生成が走らないが、次回 GUI 起動時の起動処理で消化されるため実害は小さい。

**読み取り系（Risk::Read）**

- `get_threads`
- `get_threads_by_project`
- `get_projects`
- `get_unread_counts`

検索はできるが一覧が見えない状態は実用に耐えないため含める。いずれも DB 直委譲で薄く、gate は素通りする。

## 排他制御

DB は単一の `Mutex<Connection>` であり、GUI プロセスと共有できない。

Stronghold については当初「ファイルロックを取る」と想定していたが、2026-07-20 の実測で**そうではない**ことが判明した。同一スナップショットを 2 インスタンスから開く検証の結果:

- 2 つ目の open は成功する（排他ロックを取らない）
- 後から `commit_with_keyprovider` した側が、先の書き込みを丸ごと上書きする
- エラーも警告も出ない

つまり GUI 起動中に CLI から認証情報を触ると、**無言でシークレットが消える**。`secure_store.rs` には過去に同種のデータ消失バグを踏んだ記録もある。

したがって排他は**アプリケーション側で明示的に行う**。データディレクトリに `pigeon.lock` を置き、`flock(2)` によるアドバイザリロックを CLI と GUI の双方が起動時に取得する。取得できなければ「Pigeon が起動中です」と明示して終了する。

ファイルの存在有無で判定してはいけない。プロセスがクラッシュするとロックファイルが残り、以後永久に起動できなくなる。flock は OS がプロセス終了時に自動解放するため、この問題が起きない。

ローカル IPC 経由で GUI にコマンドを転送して共存させる案は、IPC 層の新設を伴いスコープが大きく膨らむため採らない。実際に運用上困った時点で改めて検討する。

## テスト

- gate マトリクスに `Cli` の TTY / 非TTY 2 ケースを追加する。既存の Ui/Mcp/Agent のテストと同じ形式に揃える
- TTY 判定は関数注入で差し替え可能にし、テストから両分岐を検証する。実際の端末状態に依存させない
- Registry の列挙 API と Schema 導出のユニットテスト
- 新規に載せ替える 6 つの UseCase は、既存の載せ替え済み UseCase と同じテスト形式に揃える

## 既知の制限

**承認キューを消費する導線が存在しない。** CLI（非TTY）と MCP から Sensitive 操作を呼ぶと承認キューに積まれるが、それを承認する UI は未実装である（BACKLOG 5-2）。実質的な影響として、**エージェントからのメール送信・案件削除などは完了できない**。

これは本設計のスコープ外とする。承認 UI は BACKLOG 5-2 として残し、将来対応する。Read / Reversible の操作（同期・分類・検索・既読・アーカイブ・案件作成など）は承認を要さないため、エージェントからの主要な利用は今回の範囲で成立する。

## スコープ外

- 残り約 46 個の Tauri command の載せ替え
- 承認キューの消費 UI・承認後の再実行（BACKLOG 5-2）
- 常駐エージェント（BACKLOG 5-3）
- GUI と CLI の同時実行（IPC 転送による共存）

## 使い方

### CLI

    pigeon-cli --help                              # サブコマンド一覧
    pigeon-cli projects <account_id>               # 案件一覧
    pigeon-cli threads <account_id> [folder]       # スレッド一覧（既定 INBOX）
    pigeon-cli unread <account_id>                 # 未読件数
    pigeon-cli sync <account_id>                   # 同期（進捗は stderr）
    pigeon-cli search <account_id> "<query>"       # 全文検索
    pigeon-cli call --list                         # 呼べる UseCase と入力スキーマ
    pigeon-cli call <name> '<json>'                # UseCase を直接呼ぶ
    pigeon-cli driver                              # 判定された driver を表示
    pigeon-cli <任意のコマンド> --json             # 機械可読出力

`driver` と `call --list` は Registry だけで応答できるため、DB も SecureStore も開かない。GUI 起動中でも実行できる。

### MCP

MCP クライアントの設定に次を追加する。

    { "command": "pigeon-cli", "args": ["mcp"] }

`initialize` / `tools/list` / `tools/call` の 3 メソッドに対応する。tools は Registry から自動導出されるため、UseCase をバスに載せれば設定を変えずに露出する。

driver は TTY 判定ではなく `Driver::Mcp` を固定で使う。MCP 経由であることを監査ログに残すためで、`pigeon-cli mcp` が CLI プロセスであっても `CliAutomated` にはしない。

ランタイム（DB / SecureStore）は `tools/call` で初めて開く。`initialize` と `tools/list` は tool 一覧を返すだけで DB もキーチェーンも要らないため、クライアントの接続確認だけで GUI とのロック競合を起こさない。

stdout は JSON-RPC が占有する。進捗は `NoOpProgressSink` に落とし（Ctx に progress を設定しない）、診断出力はすべて stderr へ書く。

UseCase がエラーを返した場合は JSON-RPC のエラーではなく、`isError: true` を含む成功レスポンスで返す（MCP 仕様。プロトコルエラーと UseCase エラーは別物）。ランタイムを開けない場合（GUI 起動中など）はプロトコル以前の環境エラーなので JSON-RPC エラー（`-32603`）で返す。
