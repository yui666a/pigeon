# スター/フラグ表示・付与・解除 と 手動で未読に戻す

バックログ A1（スター/フラグ）・A2（手動で未読に戻す）を実装する。
既読/未読の既存設計（`2026-07-12-read-unread-design.md`）と同じ「DB即時更新 + サーバーへ
バックグラウンドでベストエフォート反映」の経路を、IMAP フラグ `\Flagged` にも適用する。

## A1. スター / フラグ

### データモデル

- `mails.is_flagged INTEGER NOT NULL DEFAULT 0`（migrate_v9）
- 正はサーバー側の `\Flagged`。ローカルの `is_flagged` は既存の `flags` TEXT カラムと同様に
  そのキャッシュ。マイグレーション時、既存の `flags` に `\Flagged` を含む行は `is_flagged=1`
  に埋め戻す（`is_read` を `flags` の `\Seen` から埋め戻さなかったのは v7 時点で `flags` の
  一括再取得が未実装だったため。今回は文字列に情報があるので活かす）

### サーバー → ローカル（フラグ取り込み）

既読/未読と同じ経路を流用する:

1. **新規メール**: `fetch_mails_batched` は既に `FLAGS` を取得しているため、
   `parse_mime` で `\Flagged` の有無を `is_flagged` として保存する
   （`is_read` と同様、`contains_seen` に倣った `contains_flagged` を追加）
2. **既知メール**: `fetch_seen_map` を拡張し、`(uid) -> (is_read, is_flagged)` を
   返すようにする（`fetch_seen_map` → 汎用的な `fetch_flag_map` に改名し、
   `mail_commands::sync_account_inner` 側で `is_read` と `is_flagged` を同時に UPDATE する）

既読/未読とフラグは同じ FLAGS FETCH の1回で両方取れるため、専用の別 FETCH は追加しない。

### ローカル → サーバー（set_flagged）

- 新規ファイル `src-tauri/src/commands/flag_commands.rs` に `set_flagged(account_id, mail_id, flagged: bool)`:
  1. DB の `is_flagged` を即時更新（UI はこれで確定）
  2. IMAP `UID STORE +FLAGS.SILENT (\Flagged)` / `-FLAGS.SILENT (\Flagged)` を
     `tauri::async_runtime::spawn` のバックグラウンドタスクでベストエフォート実行する
- Sent 等 `LocalOnly` フォルダ（`mail_commands::plan_delete` が `LocalOnly` とする `Sent`）は
  ローカル更新のみでサーバー反映をスキップする。判定は既存の `plan_delete` とは意味が異なる
  （削除/アーカイブの「サーバー反映方式」ではなく「サーバーUIDを信頼できるか」）ため、
  `flag_commands.rs` 内に `is_local_only_folder(folder: &str) -> bool`（`folder == "Sent"`）を
  独自に定義する。mail_commands.rs は変更しない

### UI

- `MailActions.tsx` にスタートグルボタン（★/☆）を追加
- `ThreadItem.tsx` にスレッド内の付いているメールを含む場合の★マークを追加
  （`thread.mails.some((m) => m.is_flagged)`。`hasUnread` の判定と同じ形）
- 一覧の構造変更はしない。表示の追加のみ

### v1 の制限

- Sent はサーバー反映しない（uid 不一致問題。既存の delete/archive と同じ制限）
- フラグの色・種類（IMAP には `\Flagged` 以外のカスタムフラグもあるが）は単純な★のみ。
  Gmail の複数色ラベルには対応しない

## A2. 手動で未読に戻す

### ローカル → サーバー（mark_unread）

- `flag_commands.rs` に `mark_unread(account_id, mail_id)` を追加。`mark_read` の逆:
  1. DB の `is_read = 0` を即時更新
  2. IMAP `UID STORE -FLAGS.SILENT (\Seen)` をバックグラウンドでベストエフォート実行

### UI と自動既読化の干渉回避

`mailStore.selectMail` / `selectThread` は選択時に未読なら自動で `markMailRead` を呼ぶ。
未読化した直後にそのメールが選択されたままだと、選択状態の変化なしに次の描画で
再度「未読 → 選択中」の判定に引っかかることはない（`selectMail`/`selectThread` は
"選択操作" 時にのみ既読化を判定するため、選択済みのメールを未読化しても再選択が
発生しない限り再既読化されない）。ただし表示上は「本文を表示したまま未読マークが付く」
という一貫性のない状態になるため、v1 では **未読にする操作をしたら選択を解除する**
（`selectMail(null)` して一覧に戻す）。これにより:

- 未読化した直後に自動既読化が起きない（選択解除により判定自体が走らない）
- ユーザーから見ても「未読にしたら一覧に戻る」という Gmail 等と同様の自然な挙動になる

### UI

- `MailActions.tsx` に「未読にする」ボタンを追加

## 両機能共通

- マイグレーション番号は v9（並行開発中の他エージェントが v10/v11 を使用するため固定）
- `lib.rs` invoke_handler への追加は `mark_read` の直後
- `commands/mod.rs` への追加は先頭付近
- mailStore.ts の変更はアクション追加のみ（既存アクションの変更禁止）
