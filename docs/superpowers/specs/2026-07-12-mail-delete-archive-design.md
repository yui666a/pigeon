# メール削除・アーカイブ（サーバー反映）設計書

日付: 2026-07-12
ステータス: 実装対象
関連: `2026-04-12-pigeon-design.md`「4. メール同期 / 双方向同期」「キーボードショートカット」

## 目的

メールの削除とアーカイブを IMAP サーバーへ反映する。既読反映（`mark_read`）は
「DB 即時 + サーバーはベストエフォート」だが、削除は破壊的操作であり失敗の黙殺は
危険なため、**サーバー処理を同期的に実行し、成功した場合のみローカルへ反映する**。

## 操作の定義

### 削除（delete_mail）

削除は「ゴミ箱への移動」を第一義とする。Gmail では `\Deleted` + EXPUNGE が
「INBOX ラベル剥がし（=アーカイブ相当）」にしかならないため、ゴミ箱フォルダへの
COPY を先に行うことで、**削除とアーカイブがメール1件ごとに選べる別操作**になる。

1. サーバー: LIST `*` の応答から SPECIAL-USE (RFC 6154) の `\Trash` 属性を持つ
   フォルダを探す（Gmail ならロケールに依らず `[Gmail]/ゴミ箱` 等が見つかる）
   - **見つかった場合**: 対象フォルダを SELECT → `UID COPY <uid>` でゴミ箱へ →
     `UID STORE <uid> +FLAGS.SILENT (\Deleted)` → EXPUNGE（= ゴミ箱へ移動）
   - **見つからない場合**（SPECIAL-USE 非対応のセルフホスト等）: 従来どおり
     `\Deleted` + EXPUNGE のみ（= 完全削除）
2. 成功したらローカル DB から行を DELETE（`mail_project_assignments` /
   `mail_attachments` / `correction_log` は CASCADE、FTS はトリガーで削除）
3. サーバー処理が失敗したらエラーを返し、ローカルは変更しない

### アーカイブ（archive_mail）

削除と違い**ローカル行は消さない**。`mails.folder` を `'Archive'` に更新することで
INBOX の一覧からは消えるが、案件割り当て・スレッド・検索は維持される（これが価値）。

サーバー側の処理はプロバイダで分岐する:

| provider | サーバー処理 |
|----------|--------------|
| google | COPY せず `\Deleted` + EXPUNGE のみ。Gmail では INBOX からの削除は「INBOX ラベルを剥がす」ことと等価で、メールは All Mail に残る（= アーカイブ）。`[Gmail]/All Mail` への COPY は不要 |
| other | settings の `archive_folder`（デフォルト `"Archive"`）へ `UID COPY`。フォルダが無く COPY が失敗した場合は `CREATE` を試みて 1 回だけ再試行 → その後 `\Deleted` + EXPUNGE |

成功したらローカルの `mails.folder` を `'Archive'` に更新する。

## EXPUNGE の方式（UIDPLUS）

`UID EXPUNGE`（RFC 4315 / UIDPLUS 拡張）が使えるサーバーでは対象 UID のみを
EXPUNGE する。async-imap 0.11 は `Session::uid_expunge` と `capabilities()` を
提供しており、**CAPABILITY に `UIDPLUS` があれば `UID EXPUNGE <uid>`、なければ
通常の `EXPUNGE`** にフォールバックする。

注意点: 通常 EXPUNGE はフォルダ内の `\Deleted` 付き全メールを削除する。本アプリは
対象 UID にしか `\Deleted` を付けないため通常は実害がないが、他クライアントが
`\Deleted` を付けて EXPUNGE していないメールが存在する場合、それらも同時に消える。
これは仕様上の制約として許容する（Gmail ほか主要サーバーは UIDPLUS 対応）。

## モジュール構成

### imap_client.rs（mail_sync）

- `delete_message(session, folder, uid)`: SELECT → `+FLAGS.SILENT (\Deleted)` → EXPUNGE（上記方式）
- `copy_message(session, folder, uid, dest)`: SELECT → `UID COPY`。失敗時 `CREATE dest` → 再試行
- 接続込みラッパー（既存 `append_message` のパターン）:
  - `delete_message_remote(host, port, auth, user, cred, folder, uid)`
  - `archive_message_remote(..., folder, uid, copy_dest: Option<&str>)` — `copy_dest` が
    `Some` なら COPY してから削除（other）、`None` なら削除のみ（google）

### commands/mail_commands.rs

- `delete_mail(account_id, mail_id)` / `archive_mail(account_id, mail_id)`
- サーバー反映の要否・方式は純粋関数 `plan_delete(folder)` / `plan_archive(provider,
  folder, archive_folder)` で決定する（単体テスト対象）
- アカウント不在は `AccountNotFound`、メール不在は `MailNotFound` を返す
- `lib.rs` の invoke_handler 末尾に登録

### db/mails.rs

- `delete_mail(conn, mail_id)`: 行を削除。対象が無ければ `MailNotFound`
- `update_folder(conn, mail_id, folder)`: フォルダ更新。対象が無ければ `MailNotFound`

## フロントエンド

- `MailActions.tsx`: 「アーカイブ」「削除」ボタンを追加。削除は `window.confirm` で確認
- `mailStore.ts`: `deleteMail(mail)` / `archiveMail(mail)` アクション。invoke 成功後に
  threads / selectedThread / selectedMail / unclassifiedMails から該当メールを除去
  （スレッドが空になればスレッドごと除去）し、`fetchUnreadCounts` を呼ぶ。
  成功時は成功トースト（`errorStore.addSuccess`。「削除しました」/「アーカイブしました」）で
  フィードバックする。失敗時は `errorStore.addError` のみでローカル状態は変更しない
- `useKeyboardShortcuts.ts`: `e` = アーカイブ（既存の発火ガード —
  修飾キー・テキスト入力中・コンポーズ表示中は無効 — を踏襲）

## エラー・順序の原則

- **サーバー成功 → ローカル反映** の順序を厳守する（楽観更新しない）
- サーバー処理中は DB ロックを保持しない（読み取り → ロック解放 → IMAP → 再ロックして反映）
- IMAP logout の失敗は警告ログのみ（既存パターン踏襲）

## v1 の制限

- **Sent フォルダのメール**: 送信時に APPEND したローカル行の `uid` は
  `get_max_uid + 1` の推定値でありサーバー UID と不一致の可能性があるため、
  v1 ではサーバー反映を行わない。削除はローカル行の削除のみ、アーカイブは
  ローカルの folder 更新のみ（`plan_* = LocalOnly`）
- アーカイブの取り消し（Unarchive）は未対応
- 複数選択の一括削除・一括アーカイブは未対応
- UIDPLUS 非対応サーバーでは通常 EXPUNGE を使う（上記注意点）
