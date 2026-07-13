# 過去メールのバックフィル 設計書

日付: 2026-07-13
ステータス: 実装対象
関連: `docs/BACKLOG.md` 項目8 / `2026-07-09-bulk-initial-sync-design.md`（将来リスト「既存アカウントの過去遡り」）

## 目的

初回同期は `settings.initial_sync_limit`（既定5,000件）より古いメールを取り込まない。
このため案件分類の対象になる古いメールがローカルに存在しないアカウントができる。
ユーザーが明示的に操作したときだけ、ローカル最古メールより古いメールを一定件数
遡って取得する機能を追加する。

## 方針: 既存バッチ経路を逆方向に使う

`fetch_mails_batched`（`imap_client.rs`）は「`since_uid` より新しい範囲」を昇順で
取得する一方向の設計（初回同期・差分同期の両方がこれに乗る）。バックフィルは
「ローカル最古 UID 未満の範囲」を対象にする点で本質的に逆方向であり、既存関数の
`since_uid` パラメータでは表現できない。そのため新しい関数
`fetch_mails_backfill_batched` を追加し、バッチ分割・進捗通知・DB挿入経路
（`sync_folder_into` 相当のロジック）は可能な限り共通化する。

### 対象範囲

- **INBOX のみ**（Sent のバックフィルはスコープ外。Sent は差分同期対象がそもそも
  `initial_sync_limit` と同じ上限で日常的に十分カバーされ、優先度が低いため）
- ローカルの `min_uid = get_min_uid(account, "INBOX")` **未満**のサーバー UID が対象。
  `min_uid` が 0（メールが1件もない）場合はバックフィル自体が無意味なので早期リターンする
  （通常の初回同期に任せる）
- 取得順は「新しい→古い」（サーバー UID 降順）。1回のボタン操作で `limit` 件
  （既定は `settings.initial_sync_limit` と同じ値をフロントから渡す。設定UIは作らない）
- `limit` 件に満たない場合や UID 1 まで到達したら、それ以上古いメールがないと判定する

## バックエンド

### db/mails.rs: `get_min_uid`

```rust
pub fn get_min_uid(conn: &Connection, account_id: &str, folder: &str) -> Result<u32, AppError>
```

`folder` 内の最小 uid を返す。行が無ければ 0（`get_max_uid` と対称の実装）。

### mail_sync/imap_client.rs: バックフィル用バッチ計画

```rust
/// min_uid_exclusive 未満の UID を降順（新しい→古い）に並べ、先頭 limit 件までを
/// batch_size ごとに分割する。バッチ内部の並びは降順を維持する
/// （サーバー FETCH のレンジ指定にそのまま使うため）。
pub(crate) fn plan_backfill_batches(
    uids: Vec<u32>,
    min_uid_exclusive: u32,
    limit: usize,
    batch_size: usize,
) -> Vec<Vec<u32>>
```

- `min_uid_exclusive` 以上の UID は除外（重複取得の防止。境界は `<`）
- 降順ソート・重複除去後、先頭 `limit` 件だけを対象にしてからバッチ分割する
  （`limit` 件超のバッチを作らないよう、切り詰めをバッチ分割の**前**に行う）
- 既存 `plan_batches` は昇順・下限フィルタ・件数無制限のままで変更しない
  （差分同期の挙動に影響を与えない）

### mail_sync/imap_client.rs: `fetch_mails_backfill_batched`

```rust
pub async fn fetch_mails_backfill_batched(
    session: &mut ImapSession,
    folder: &str,
    min_uid_exclusive: u32,
    limit: u32,
    mut on_batch: impl FnMut(Vec<FetchedMail>, SyncProgress) -> Result<(), AppError>,
) -> Result<BackfillResult, AppError>
```

- UID一覧を `1:(min_uid_exclusive - 1)` の範囲で軽量 FETCH（`min_uid_exclusive <= 1` なら
  対象なしとして即 `Ok` を返す。`min_uid_exclusive` は呼び出し前に `get_min_uid` の結果を
  渡すため 0 のケースは呼び出し側で弾く）
- `plan_backfill_batches` でバッチ計画し、各バッチを `uid_set` で `UID FETCH (UID FLAGS RFC822)`
- `BackfillResult { fetched: usize, exhausted: bool }` を返す。`exhausted` は
  「`limit` 未満しか対象 UID がなかった＝これ以上古いメールがサーバーにない」を表す
  （`plan_backfill_batches` に渡す前の「フィルタ後・切り詰め前」の総数が `limit` 未満かで判定）

> **追記（2026-07-13 リファクタリング）**: `backfill_account_inner` / `backfill_folder_into` は
> `mail_sync/sync_service.rs` へ移動した（`sync_folder_into` 等の通常同期ロジックと同居）。
> `backfill_account` コマンド自体（SyncLocks 共有・`backfill-progress` emit・
> `BackfillOutcome` 返却）は引き続き `commands/mail_commands.rs`。

### commands/mail_commands.rs: `backfill_account`

```rust
#[tauri::command]
pub async fn backfill_account(
    app: AppHandle,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    limit: u32,
) -> Result<BackfillOutcome, AppError>
```

- `SyncLocks` を**通常同期と共有**する（`try_begin`/`finish` は account_id 単位のため、
  同一アカウントの通常同期とバックフィルが同時に同じ IMAP セッション操作・DB書き込みを
  行うことを防ぐ）。ロック中なら同期と同じく「実行しない」を表す結果を返す
  （エラーにはしない。呼び出し側でボタンを無効化する主対策と合わせた保険）
- 進捗イベントは既存 `sync-progress` を再利用せず、**別イベント `backfill-progress`**
  を emit する。理由: 同一アカウントで通常同期の自動トリガー（IDLE 検知後の再同期等）と
  バックフィルの進捗表示が同時に走ると、`sync-progress` 1本では「今どちらの進捗か」を
  UI が区別できない。`SyncProgressEvent` と同型のペイロードに `account_id, done, total` を持つ
- `BackfillOutcome { fetched: u32, exhausted: bool }` を返す。フロントはこれで
  「もう遡るものがない」を判定しボタンを無効化する

内部実装は `sync_folder_into` と同じ「バッチ受け取り→ロック取得→`parse_mime`→
`insert_mail`（`MergeStrategy::InsertOrIgnore`）」を行う小関数
`backfill_folder_into` を新設し、`fetch_mails_backfill_batched` を呼ぶ。
`sync_folder_into` 自体は変更しない（前方同期の経路に影響を与えないため）。

### invoke_handler 登録

`lib.rs` の `sync_account` の直後に `commands::mail_commands::backfill_account` を追加する。

## フロントエンド

### mailStore.ts への追加（新規アクションのみ・既存アクション変更禁止）

```ts
interface MailState {
  // ...
  backfilling: boolean;
  backfillProgress: { account_id: string; done: number; total: number } | null;
  backfillExhausted: Record<string, boolean>; // account_id -> これ以上遡れないか
  backfillAccount: (accountId: string, limit: number) => Promise<void>;
  initBackfillListener: () => Promise<() => void>;
}
```

- `backfillAccount`: `backfilling` 中は即 return（多重実行防止。UI 側のボタン無効化と
  二重の防御）。成功したら `backfillExhausted[accountId] = outcome.exhausted` を記録し、
  現在表示中のビューを再取得する（`syncAccount` 完了時の再取得ロジックと同じ関数を再利用）
- `initBackfillListener`: `backfill-progress` を購読し `backfillProgress` を更新する。
  `SyncIndicator` の `initSyncListener` と同型

### UI

- `AccountList.tsx` の各アカウント行に「過去のメールを取得」ボタンを追加
  （再認証ボタンと同じ並びの操作ボタン群に追加。`account.needs_reauth` のときは表示しない
  ＝再認証が先という既存の優先順位に合わせる）
- 実行中は同ボタンを disabled にしてラベルを「取得中…」に変える
  （`backfilling` を見る。既存の `syncing` とは独立したフラグのため、通常同期中でも
  バックフィルボタンの活性状態自体は変えない。ただし `SyncLocks` 共有によりバックエンド側で
  実際には弾かれるため、二重実行は起きない）
- `backfillExhausted[account.id]` が true ならボタンを disabled にし「全件取得済み」表示に変える
- 進捗は `SyncIndicator` に「過去メール取得中… n / total」の行を追加する形で表示する
  （新規コンポーネントは作らず、既存コンポーネント内で `backfillProgress` の有無により
  通常同期の行と切り替え表示。同時に両方が進行することは `SyncLocks` 共有により起きない）

## v1 の制限（スコープ外）

- **Sent のバックフィルは対象外**。将来必要になれば `backfill_folder_into` に
  `MergeStrategy::UpsertByMessageId` を渡す経路を追加すれば拡張できる
  （`sync_sent_folder` と同じ構造）
- **バックフィル件数の設定UIは作らない**。フロントから渡す `limit` は
  `settings.initial_sync_limit` の値をそのまま流用する
- **自動トリガーはしない**。ユーザーがボタンを押した時のみ実行する
- **アカウント削除・切断時の途中終了からの再開制御はしない**。バッチ単位で DB へは
  確定済みのため中断しても取り込み済み分は残るが、「あと何件残っているか」の状態は
  永続化しない。次にボタンを押すと、その時点の `min_uid` を起点に再度 `limit` 件を遡る
  （初回同期と同じ「バッチ単位で確定・状態はDBの実データのみで表現する」設計を踏襲）
