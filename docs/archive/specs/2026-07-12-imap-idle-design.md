# IMAP IDLE によるリアルタイム同期

2026-04-12 設計書「4. メール同期」の「IMAP IDLE で待機（プッシュ通知相当）」
「接続断時は exponential backoff で自動リトライ」を実装する。
既読/未読同期（2026-07-12-read-unread-design.md）の上に積む。

## 方針: 検知と取り込みの分離

バックエンドの IDLE 監視タスクは**新着の検知だけ**を行い、
実際の取り込みは既存の `sync_account` 経路を再利用する。

1. アカウント毎に独立した監視タスク（tokio task）が専用 IMAP 接続で INBOX を IDLE 監視する
2. 新着を意味する untagged response（`* n EXISTS` / `* n RECENT`）を検知したら
   Tauri イベント `new-mail-detected` `{ account_id }` を emit するだけ
3. フロントエンドがこのイベントを受けて既存の `syncAccount(accountId)` を呼ぶ

この設計の利点:

- 取り込みロジックの重複がない。多重実行は既存の SyncLocks（バックエンド）と
  `syncing` フラグ（フロントエンド）のガードがそのまま効く
- 取り込み後の一覧反映・未読件数更新は既存の `sync-progress` フローに乗る
- 将来のデスクトップ通知も同じ `new-mail-detected` イベントに載せられる

## バックエンド

### `mail_sync/idle.rs`（新規）

- `watch_inbox(app, account_id)`: connect → CAPABILITY 確認 → SELECT INBOX → IDLE ループ
- RFC 2177 はサーバーの無通信切断（30分）対策として 29 分以内の再発行を推奨。
  余裕を持って **25 分ごとに IDLE を張り直す**（`IDLE_REFRESH_INTERVAL`）
- 新着判定は純関数 `is_new_mail_response(&Response)`（EXISTS / RECENT のみ true。
  EXPUNGE や FETCH 等のフラグ変更通知では同期を起動しない）
- 監視セッションの終わり方を `SessionOutcome` に分類し、
  再接続ループ `watch_loop`（セッション実行と sleep を注入可能）が解釈する:
  - `Disconnected`（接続確立後の切断）: backoff をリセットして再接続
  - `ConnectFailed`（接続・認証失敗）: exponential backoff で再接続
    （30s → 60s → 120s → … 最大 10 分。`next_backoff` 純関数）
  - `Stop`（`ReauthRequired` / アカウント削除）: ログを出して監視を終了。
    再認証後の再開は OAuth 完了時の再スタートに任せる（v1 ではそれ以上追わない）
- CAPABILITY に IDLE がないサーバーは、接続を切って **15 分間隔のポーリング**
  （`new-mail-detected` の emit のみ。取り込みはやはり sync_account 側）にフォールバック

### 状態管理・ライフサイクル

- `IdleWatchers`（state.rs）: `Mutex<HashMap<account_id, JoinHandle>>`。
  `insert`（既存タスクは abort して置換）/ `stop`（abort + 削除）で開始・停止を管理
- 開始: アプリ起動時（lib.rs setup で全アカウント）、`create_account`、
  OAuth 完了時（`handle_oauth_callback` 成功時。再認証後の再開もここで置換される）
- 停止: `remove_account`
- commands からは `idle::start_watching(app, account_id)` / `idle::stop_watching(app, account_id)`
  ヘルパー経由で操作する
- 監視タスク内ではエラーを Result / SessionOutcome で処理し panic させない
  （unwrap / expect 禁止の規約どおり）

## フロントエンド

- mailStore に `initNewMailListener` を追加（`initSyncListener` と同じパターン）:
  `new-mail-detected` を listen し、該当アカウントの `syncAccount(account_id)` を呼ぶ
- `selectedAccountId` と無関係に同期してよい（同期中なら既存ガードで 0 が返るだけ。
  一覧への反映可否は既存の sync-progress リスナーが表示中アカウントを見て判断する）
- リスナー登録は App.tsx の useEffect で行う（アプリ全体の関心事のため）

## テスト

- `next_backoff`: 2 倍・10 分キャップ
- `is_new_mail_response`: EXISTS / RECENT で true、EXPUNGE / FETCH 等で false
- `watch_loop`: セッション実行と sleep をクロージャ注入し、
  正常→切断→backoff→復帰→停止の状態遷移と sleep 間隔を検証
- `IdleWatchers`: 開始・置換・停止の管理
- フロント: `new-mail-detected` 受信で `sync_account` が invoke されること
- 実 IMAP 接続・実 IDLE は統合境界として自動テスト対象外
