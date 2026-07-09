# 初回同期の大量取り込み（Bulk Initial Sync） 設計書

## 概要

アカウント追加時の初回同期で取り込むメール件数を、現行の直近20件から**既定5,000件**に拡大する。あわせて同期処理をバッチ化し、進捗表示・中断再開・大量件数でも固まらない一覧表示を実現する。

### 背景

現行の初回同期は `INITIAL_SYNC_LIMIT = 20` 件のみで、案件分類の対象になる過去メールがほとんど取り込まれない。単純に定数を5,000へ変えると、1回の IMAP FETCH で添付含む全文（RFC822）×5,000件を一括取得・全件メモリ保持する現実装では通信量・メモリ・応答時間が破綻するため、取得経路の再設計が必要。

### スコープ

**含む:**

- 初回同期件数の設定値化（`settings.initial_sync_limit`、既定 5000。設定UIは作らない）
- UID一覧の軽量取得 → 100件単位のバッチ FETCH → バッチごとの DB 挿入への変更（差分同期も同一経路に統一）
- 古い順の処理による中断再開（途中終了しても次回同期が続きから取り込む）
- Tauri イベントによる同期進捗通知とサイドバー下部の進捗表示（`SyncIndicator`）
- スレッド一覧・未分類一覧の表示ページング（先頭200件 + 「もっと見る」）

**含まない（将来リスト）:**

| 機能 | 備考 |
|------|------|
| 既存アカウントの過去遡り（バックフィル） | 初回同期のみ対象。遡りボタンは将来 `since_uid` 未満の範囲を同じバッチ経路で取るだけなので拡張余地は確保される |
| 取り込んだ大量メールの自動AI分類 | 分類は従来どおり手動トリガー（Ollama で5,000件は非現実的） |
| 同期件数の設定変更UI | settings 値として持つのみ。変更は将来の設定画面で |
| サイズ上限による本文スキップ（ヘッダ先行2段階取得） | 「本文未取得」状態の管理が複雑なため見送り。同期速度が問題になったら再検討 |

## 1. 現状の構造と問題

```
sync_account (commands/mail_commands.rs)
  └─ fetch_mails_since_uid (mail_sync/imap_client.rs)
       ├─ 初回(since_uid=0): 直近20件のシーケンス範囲を 1回の FETCH (UID RFC822)
       ├─ 差分: since_uid+1:* を 1回の UID FETCH (UID RFC822) ← 件数無制限
       └─ 全件を Vec<(u32, Vec<u8>)> でメモリ保持
  └─ 全件パース → 1つのロック内で全件 insert_mail
```

問題点:

1. 全文一括取得・全件メモリ保持のため、件数を増やすとメモリと通信がスケールしない
2. 差分同期も無制限一括のため、長期間未起動のアカウントで同じ問題を踏む
3. 完了までUIに何も反映されず、進捗も出ない。途中でアプリが落ちると全部やり直し

## 2. 設計

### データフロー（バッチ同期）

```
sync_account(account_id)
    │
    ▼
SELECT INBOX → mailbox.exists から対象シーケンス範囲を決定
    │            (初回: 直近 initial_sync_limit 件 / 差分: since_uid より新しい範囲)
    ▼
UID一覧のみ軽量 FETCH（"(UID)"、本文なし）→ since_uid より大きい UID を昇順ソート
    │
    ▼ 100件ずつ（SYNC_BATCH_SIZE）、古い順
UID FETCH (RFC822) → parse_mime → insert_mail（バッチ単位でロック取得）
    │
    ├─ バッチ完了ごとに Tauri イベント "sync-progress" を emit
    │     payload: { account_id, done, total }
    ▼
完了 → 取り込み件数を返す（現行の戻り値と同じ）
```

### 設計原則

- **古い順に処理する**: `max_uid`（DB内の最大UID）が常に「ここまで取り込み済み」を意味するため、途中でアプリが落ちても次回の `sync_account` が残りを差分として自然に継続する。専用の再開状態・リトライ管理を持たない
- **メモリはバッチサイズで一定**: 全文を保持するのは1バッチ（100件）分のみ
- **初回と差分で経路を分けない**: どちらも「UID一覧 → バッチFETCH」の同一経路。差分が大量（長期未起動）でも同じ性質で動く

### 定数・設定

| 項目 | 値 | 置き場所 |
|------|-----|---------|
| 初回同期の最大件数 | 既定 5000 | `settings.initial_sync_limit`（`get_or_default` で読む。設定UIなし） |
| バッチサイズ | 100 | `imap_client.rs` の定数 `SYNC_BATCH_SIZE` |
| 進捗イベント名 | `sync-progress` | バッチごとに emit |

`INITIAL_SYNC_LIMIT` 定数は廃止し、呼び出し側（`sync_account_inner`）が settings から読んだ値を渡す。

### インターフェース変更（Rust）

```rust
// imap_client.rs
pub struct SyncProgress { pub done: usize, pub total: usize }

/// UID一覧を取得し、バッチごとに fetch → on_batch コールバックを呼ぶ。
/// on_batch は (そのバッチの生メール, 進捗) を受け取る。
pub async fn fetch_mails_batched(
    session: &mut ImapSession,
    folder: &str,
    since_uid: u32,
    initial_limit: u32,
    mut on_batch: impl FnMut(Vec<(u32, Vec<u8>)>, SyncProgress) -> Result<(), AppError>,
) -> Result<u32, AppError>  // 取り込み対象の総件数を返す
```

- `sync_account_inner` は `on_batch` 内でパース・DB挿入・`app_handle.emit("sync-progress", ...)` を行う
- `sync_account` command は `AppHandle` を受け取る形に変更（`tauri::command` の引数追加のみ）
- 既存の `fetch_mails_since_uid` は削除し、テストも新関数に置き換える

### フロントエンド

**進捗表示（`SyncIndicator`）**

- `src/components/sidebar/SyncIndicator.tsx` を新規作成（`ScanIndicator` と同型・並び）
- `mailStore` に `syncProgress: { accountId: string; done: number; total: number } | null` を追加
- マウント時に `listen("sync-progress", ...)` で購読（`initDeepLinkListener` と同じ既存パターン）。同期完了（`syncAccount` の resolve）で `null` に戻す
- 表示: 「メール同期中… 1,200 / 5,000」

**一覧の順次反映**

- `syncAccount` 実行中、進捗イベント受信のたびに現在のビュー（スレッド一覧/未分類一覧)を再取得はしない。**進捗イベント5回に1回（=500件）程度で再取得**し、DB読み出しの無駄打ちを抑える。完了時に必ず最終再取得
- 実装は mailStore 内（イベントハンドラで `done % 500 === 0` 相当の判定）

**表示ページング（固まり防止）**

- `ThreadList` / `UnclassifiedList` は全件 `.map` レンダリングのため、5,000件でDOMが固まる
- 両リストに**先頭200件表示 + 「もっと見る」ボタン（+200件ずつ）**の単純ページングを入れる。仮想化ライブラリは導入しない
- データ取得は現行どおり全件（Rust側 API は変更しない）。切るのは描画のみ

## 3. エラーハンドリング

| 状況 | 挙動 |
|------|------|
| バッチ途中で IMAP エラー | そこまでの挿入分は確定済み。エラーを返し、次回同期が続きから再開（max_uid ベース） |
| バッチ途中でアプリ終了 | 同上（挿入はバッチ単位のため中途半端な状態にならない） |
| 個別メールのパース失敗 | 現行どおりスキップして続行（`parse_mime` が None） |
| UID一覧取得で対象0件 | 進捗イベントを出さず 0 を返す（現行の空メールボックスと同じ） |
| 進捗イベントの emit 失敗 | 同期は継続（進捗表示はベストエフォート） |

## 4. テスト戦略（TDD）

### Rust

- `fetch_mails_batched` のバッチ分割ロジック: UID一覧が `SYNC_BATCH_SIZE` を跨ぐときの分割数・**昇順（古い順）処理**・`since_uid` フィルタ（純粋ロジック部を関数に切り出してテスト。IMAP セッションはテスト境界の外）
- 中断再開: 途中バッチまで挿入済みの DB 状態を作り、`max_uid` が再開点として機能すること（db レイヤのテスト）
- `sync_account_inner` の進捗コールバック発火回数（モック化した on_batch で検証）

### React（Vitest + RTL)

- `SyncIndicator`: `syncProgress` があるとき「n / total」表示、null で非表示
- `ThreadList` / `UnclassifiedList`: 201件以上で「もっと見る」が出る・クリックで追加表示・200件以下では出ない
- `mailStore`: `sync-progress` イベントで `syncProgress` が更新され、`syncAccount` 完了で null に戻る

## 5. 実装フェーズ（PR 分割の目安）

1. バックエンド: バッチ同期（`fetch_mails_batched` + settings + 進捗 emit）
2. フロントエンド: `SyncIndicator` + 一覧ページング + 順次反映

Stacked PR として依存を明記する。
