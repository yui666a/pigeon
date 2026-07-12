# 既読/未読の管理と IMAP フラグ同期

2026-04-12 設計書「4. メール同期 / 双方向同期」のうち、
「既読にする → IMAP STORE で \Seen」「フラグ変更 → ローカルDB更新」を実装する。

## データモデル

- `mails.is_read INTEGER NOT NULL DEFAULT 0`（migrate_v7）
- `mails.flags` には FETCH 時のサーバーフラグ文字列（`\Seen \Answered` 等）を保存する
- 正はサーバー側の `\Seen`。ローカルの `is_read` はそのキャッシュ

## サーバー → ローカル（フラグ取り込み）

1. **新規メール**: `fetch_mails_batched` の FETCH を `(UID FLAGS RFC822)` にし、
   `\Seen` の有無を `is_read` として保存する
2. **既知メール**: 同期（`sync_account`）の最後に `UID FETCH 1:* (FLAGS)` で
   INBOX 全体の uid → `\Seen` マップを取得し、DB の `is_read` を一括 UPDATE する。
   FLAGS のみの FETCH は軽量なため全件でも許容できる。
   これにより他クライアントで既読にした変更が次回同期で取り込まれる

## ローカル → サーバー（mark_read）

- `mark_read(account_id, mail_id)` コマンド:
  1. DB の `is_read = 1` を即時更新（UI はこれで確定）
  2. IMAP `UID STORE +FLAGS.SILENT (\Seen)` を `tauri::async_runtime::spawn` の
     バックグラウンドタスクで**ベストエフォート**実行する
- サーバー反映の失敗（オフライン等）はログのみでエラーにしない。
  ローカルの既読状態は維持し、正しい状態は次回同期のフラグ再同期で収束する
- IMAP 接続は同期処理と独立の都度接続。SyncLocks は使わない
  （STORE は取り込みと違い多重実行しても冪等なため）

## 未読件数

- `get_unread_counts(account_id)`: プロジェクト毎 + 未分類の未読件数を
  SQL 集計（`folder = 'INBOX'` のみ対象）で返す
- Sidebar のプロジェクト行に未読バッジ（0件は非表示）を表示する

## UI

- 未読メールを含むスレッド行・未分類メール行は太字表示
- メール表示（selectMail / selectThread で本文が表示されたメール）時に
  `mark_read` を invoke し、ローカル state も即時 `is_read = true` にする
