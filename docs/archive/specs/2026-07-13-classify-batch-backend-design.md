# バッチ分類のバックエンド化（classify_batch）設計書

- 作成日: 2026-07-13
- ステータス: 実装済み
- 関連: `2026-04-13-phase2-ai-classification-design.md`,
  `2026-07-12-sequential-classification-design.md`（本設計で置き換え）

## 1. 目的

`2026-07-12-sequential-classification-design.md` で「新規案件提案の1件ずつ承認」を
実現した際、バッチ分類ループ自体がフロントエンド（`classifyStore.classifyNext()`）へ
移った。その結果、

- 「未分類を全件分類し、新規案件提案で一時停止し、承認後に再開する」という
  **業務ワークフローが UI 層（Zustand ストア）に居座る**
- 未分類 N 件に対して N 回の `classify_mail` invoke 往復でループが駆動される
- 多重実行ガードがフロント頼み（バックエンドに防御がない）

という構造的な問題が残った。本設計では、逐次分類のワークフロー意味論
（create でのみ停止・承認後に新案件込みで継続・キャンセル可能）を**維持したまま**、
ループをバックエンドの `classify_batch` ユースケースへ戻す。

## 2. ワークフロー意味論（維持する挙動）

2026-07-12 設計の要望 1〜3 をそのまま引き継ぐ:

| # | 挙動 | 実現方法（本設計） |
|---|------|-------------------|
| 1 | 新規案件提案（create）は 1 件ずつ。承認/却下まで次を出さない | `classify_batch` は create が出た時点でループを**停止**し、提案を戻り値で返す |
| 2 | 承認した案件は即座に左の案件一覧へ反映 | `approve_new_project` が `Project` を返す既存挙動を維持（フロントが `projectStore.addProject`） |
| 3 | 以降の分類は新案件も候補に含める | 承認後にフロントが `classify_batch` を**再 invoke**。`classify_one` はメールごとに `build_project_summaries` を読み直すため新案件が候補に入る |
| 4 | assign / unclassified は自動で次へ進む | バックエンドのループ内で継続 |
| 5 | 却下したメールは未分類のまま残し、同一バッチ内では再分類しない | バッチのキュー＋インデックスをバックエンドが保持し、再開時は**次のメールから**続行 |
| 6 | キャンセルで以降を分類しない | `cancel_classification(account_id)` がフラグを立て、次のメール処理前に中断 |
| 7 | 進捗表示（current / total） | `classify-progress` イベントを emit（ポーリングしない） |

## 3. バックエンド設計

### 3.1 バッチ状態（Tauri State）

`classifier/service.rs` に置く。`PendingClassifications` と同様プロセス内メモリ
（揮発性。アプリ再起動でバッチは消える）。

```rust
/// アカウント単位のバッチ分類の進行状態。
/// 「開始時に未分類スナップショットを取り、create 停止をまたいで
/// インデックスを保持する」ことで、却下済みメールの再分類を防ぐ。
pub struct ClassifyBatches(Mutex<HashMap<String, BatchEntry>>);

struct BatchEntry {
    queue: Vec<String>,  // 開始時点の未分類メールIDスナップショット（date DESC）
    index: usize,        // 次に分類するキュー位置
    running: bool,       // ループ実行中（多重 invoke ガード）
    cancelled: bool,     // キャンセル要求
}
```

- `try_begin`: 進行中（running）なら開始しない（`SyncLocks` と同じ発想の多重実行ガード）。
  停止中のバッチがあれば queue/index を引き継いで再開。無ければ
  `get_unclassified_mails` のスナップショットで新規開始。
- `pause`: create 停止時に index を保存し running を下ろす。
- `finish`: 完了・キャンセル・エラーでエントリを破棄。
- `cancel`: running ならフラグのみ（ループが次のメール前に検知して中断）、
  停止中（承認待ち）ならエントリを即破棄。

### 3.2 ユースケース（classifier/service.rs）

```rust
pub async fn classify_batch(
    db: &Mutex<Connection>,
    classifier: &dyn LlmClassifier,
    pending: &PendingClassifications,
    batches: &ClassifyBatches,
    account_id: &str,
    on_progress: impl Fn(usize, usize),  // (current, total)
) -> Result<ClassifyBatchOutcome, AppError>
```

1 回の呼び出しで「次の停止点（create 提案）または完了/キャンセル」まで進む。
ループの 1 件分は既存の `classify_one` をそのまま使うため、
**LLM 呼び出し中に DB ロックを保持しない**既存パターンが維持される
（`ClassifyBatches` のロックもメール間の状態更新時のみ短く取る）。

戻り値:

```rust
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClassifyBatchOutcome {
    /// キューを最後まで処理した
    Completed { done: usize, total: usize },
    /// create 提案で停止（proposal は保留中の提案。承認/却下後に再 invoke で再開）
    Paused { proposal: ClassifyResponse, done: usize, total: usize },
    /// cancel_classification により中断（バッチは破棄済み）
    Cancelled { done: usize, total: usize },
    /// 同一アカウントのバッチが実行中のため何もしなかった
    AlreadyRunning,
}
```

エラー（Ollama 未起動等）はループを中断してバッチを破棄し、`AppError` を返す
（フロントはエラー表示して状態をリセットする。従来のフロントループと同じ挙動）。

### 3.3 Tauri commands（IPC 契約）

| コマンド | 引数 | 戻り値 | 説明 |
|---------|------|--------|------|
| `classify_batch` | account_id | ClassifyBatchOutcome | 未分類バッチ分類を開始/再開し、次の停止点か完了まで進む |
| `cancel_classification` | account_id | — | 実行中/承認待ちのバッチを中止 |

`classify_batch` コマンド（commands/classify_commands.rs）は分類器の構築と
`classify-progress` イベントの emit のみを担い、ループ本体はサービス層に置く
（`sync_account` / `sync_service` と同じ分業）。

進捗イベント（sync-progress と同様ベストエフォート）:

```rust
handle.emit("classify-progress", ClassifyProgressEvent {
    account_id: "...",
    current: 3,   // 処理済み件数
    total: 15,    // バッチ開始時のキュー長（再開しても不変）
});
```

`classify-complete` イベントは設けない。完了は invoke の戻り値
（`Completed`）で伝わるため二重の通知経路を作らない。

## 4. フロントエンド設計

`classifyStore` は「1 invoke → 停止したら承認/却下 UI → 再 invoke」の薄い制御だけを持つ:

```
classifyAll(accountId):
  outcome = classify_batch(accountId)     // classifyApi 経由
  match outcome.status:
    "completed" | "cancelled" → 状態リセット
    "paused"          → pendingProposal = outcome.proposal（承認/却下待ち）
    "already_running" → 何もしない（進行中のバッチに任せる）

approveNewProject(mailId, name, desc):
  project = approve_new_project(...)      // 失敗時は提案を残して中断
  projectStore.addProject(project)
  pendingProposal = null
  classify_batch(accountId) を再 invoke   // 再開

rejectClassification(mailId):
  reject_classification(mailId)
  pendingProposal = null
  classify_batch(accountId) を再 invoke   // 再開（却下メールはスキップされる）

cancelClassification():
  cancel_classification(accountId)
  状態リセット

進捗: classify-progress イベントを購読して progress を更新
（mailStore の initSyncListener / SyncIndicator と同じ様式で
 ClassifyButton が購読を張る）
```

`_queue` / `_index` / `_cancelled` などのループ内部状態と
`get_unclassified_mails` のフロント呼び出し（`fetchUnclassifiedMailRefs`）は削除する。

## 5. 排他・ロック方針

- **DB ロック**: `classify_one` の既存方針を踏襲（入力スナップショットと永続化の
  2 回だけ短く取り、LLM 呼び出し中は保持しない）。
- **多重実行ガード**: `ClassifyBatches.try_begin` が running フラグで同一アカウントの
  並行バッチを拒否する（React 開発モードの二重 effect・二度押し対策）。
  `SyncLocks` と同様、エラーではなく `AlreadyRunning` を返して呼び出し側は無視する。
- **SyncLocks とは独立**: 分類はメール同期と資源を奪い合わない（IMAP 接続を使わず、
  DB ロックは短時間のみ）ため、同期ロックとは共有しない。

## 6. テスト方針（TDD）

### Rust（cargo test, StubLlm / SeqLlm）

- 全件 assign → `Completed`、全件割り当て済み、進捗が (1,N)…(N,N) で通知される
- create で `Paused`（proposal のメールが pending に積まれる）→ 再 invoke で
  **次のメールから**再開し `Completed`（却下/承認済みメールを再分類しない、total 不変）
- `cancel_classification` → 次のメール前に `Cancelled`、バッチ破棄
  （次回は新しいスナップショットで開始）
- エラー（LLM unhealthy）→ `Err` でバッチ破棄、次回は新規開始
- `ClassifyBatches` の状態遷移（try_begin の多重ガード / pause / finish /
  承認待ち中の cancel でエントリ破棄）

### フロント（Vitest）

- `classifyAll`: completed で状態リセット / paused で pendingProposal セット
- `approveNewProject` / `rejectClassification`: 再 invoke で再開
- `cancelClassification`: cancel_classification を invoke し、以降再開しない
- `classify-progress` イベントで progress が更新される（対象アカウントのみ）

## 7. スコープ外（YAGNI）

- 分類の並列化・N 通まとめての LLM 呼び出し（Phase 5 の申し送りのまま）
- バッチ状態の永続化（アプリ再起動で消える。`PendingClassifications` と同じ扱い）
- 同期（SyncLocks）との排他
- 却下メールの再提案抑制のバッチ横断化（新しいバッチでは再提案されうる。従来どおり）
