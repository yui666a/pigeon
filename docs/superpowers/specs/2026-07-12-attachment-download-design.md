# 添付ファイルの一覧表示・ダウンロード設計

日付: 2026-07-12
ステータス: 実装

## 目的

メールの添付ファイルをユーザーが一覧・保存できるようにする。
受信同期は現状どおり添付をダウンロードせず、DBと同期時間を軽いまま保つ。

## 方針: オンデマンド取得 + ローカルキャッシュ

- **同期時**: 添付はダウンロードしない（`mails.has_attachments` フラグのみ保持。現状維持）
- **初回表示時**: ユーザーが添付一覧を開いたときに IMAP `UID FETCH (RFC822)` で元メールを
  取得し、mail-parser で添付を抽出する
- **キャッシュ**: 抽出したバイト列をアプリデータディレクトリ
  `{data_dir}/Pigeon/attachments/{mail_id}/{sanitized_filename}` に保存し、
  attachments テーブルへ記録する
- **2回目以降**: attachments テーブルのレコードとキャッシュファイルが揃っていれば
  それを返す（IMAP接続しない）。キャッシュファイルが欠けていれば IMAP から取り直し、
  レコードを全置換する
- **保存**: バックエンドの `save_attachment` コマンドがネイティブの保存ダイアログ
  （tauri-plugin-dialog）を開き、ユーザーが選択したパスへのみキャッシュファイルを
  コピーする。IPC 境界からは保存先パスを受け取らない（フロントが任意パスを渡せると
  webview 侵害時に任意パス書き込みが可能になるため）

## データモデル

`migrate_v7` で attachments テーブルを作成する（全体設計書 §2 のスキーマに
`ON DELETE CASCADE` を加えたもの）:

```sql
CREATE TABLE attachments (
    id          TEXT PRIMARY KEY,
    mail_id     TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
    filename    TEXT NOT NULL,
    mime_type   TEXT NOT NULL,
    size        INTEGER,
    file_path   TEXT
);
CREATE INDEX idx_attachments_mail ON attachments(mail_id);
```

`file_path` はキャッシュファイルの絶対パス。ファイルが消えていた場合は
キャッシュミスとして扱い、再取得時にレコードごと置き換える。

## バックエンド

### Tauri commands（commands/attachment_commands.rs）

- `list_attachments(mail_id) -> Vec<Attachment>`
  1. `has_attachments = false` なら空Vecを即返す
  2. attachments テーブルに mail_id のレコードがあり、全レコードの `file_path` の
     ファイルが存在すればそれを返す（キャッシュヒット）
  3. キャッシュミスなら `mails` の `account_id / folder / uid` を使い IMAP から
     元メールを取得（`resolve_imap_credentials` で認証情報を解決）、添付を抽出して
     キャッシュ保存・DB記録して返す
- `save_attachment(attachment_id) -> bool`
  - バックエンドで保存ダイアログを開き、ユーザーが選択したパスへキャッシュファイルを
    コピーする。キャンセル時は `false` を返す
  - 書き込み直前に `validate_save_dest` で防御的検証を行う（絶対パス・`..`/`.`
    セグメントなし・親ディレクトリ実在・既存ディレクトリ/シンボリックリンクの
    上書き禁止）
  - キャッシュが消えていればエラー（UIから一覧を開き直すと再取得される）

### キャッシュの掃除

- メール削除（`delete_mail` / `bulk_delete_mails`）の DB 削除成功後に、
  `{data_dir}/Pigeon/attachments/{mail_id}/` をベストエフォートで削除する
  （`remove_attachment_cache`）。掃除の失敗は警告ログのみで、削除自体は
  成功扱いとする（db 層に fs 副作用は持たせず、コマンド層で行う）
- `mail_id` が単一の通常パスコンポーネントでない場合（パス区切り・`..`・
  絶対パス等）は何もしない（キャッシュルート外の削除防止）
- アカウント削除（`delete_account`）経由のキャッシュ孤児化は未対応（将来課題）

### 添付抽出（mail_sync/mime_parser.rs）

`extract_attachments(raw: &[u8]) -> Vec<ExtractedAttachment>` を追加する。
mail-parser の `attachments()` を利用し、ファイル名・MIMEタイプ・バイト列を返す純関数。

### ファイル名サニタイズ

キャッシュのファイル名は以下で正規化する:

- パス区切り（`/`, `\`）と NUL を `_` に置換
- 先頭のドットを除去（`..` などの相対パス表現と隠しファイル化を防ぐ）
- 空になった場合（ファイル名なし含む）は `attachment-{n}`（n は添付の連番）
- 同名添付の衝突は連番プレフィックス `{n}-{filename}` で回避

## フロントエンド

- `src/types/attachment.ts`: `Attachment` 型
- `src/components/mail-view/AttachmentList.tsx`: 添付セクション。
  MailBody.tsx の本文末尾に `mail.has_attachments` のときだけ組み込む
  （MailView.tsx は変更しない）
  - 「📎 添付ファイルを表示」ボタン → `list_attachments` を invoke
  - ファイル名・サイズの一覧を表示、各行に「保存」ボタン
  - 保存: `save_attachment` を invoke（保存先の選択はバックエンドのダイアログが行う）
  - ローディング表示・エラーは errorStore へ通知

## セキュリティ

- 添付データはローカルのキャッシュとユーザー指定の保存先にのみ書き出す。
  LLMには送信しない
- 保存先はバックエンドが開くダイアログでの選択に限定し、IPC 境界から保存先パスを
  受け取らない（呼び出し元の webview を信頼しない）
- キャッシュパスはサニタイズ済みファイル名のみで構成し、mail_id は UUID のため
  パストラバーサルは発生しない

## 将来課題（本設計のスコープ外）

- インライン画像（cid:）の本文内表示
- キャッシュの容量上限・削除ポリシー
- アカウント削除（`delete_account`）時のキャッシュファイル掃除
  （メール削除時の掃除は「キャッシュの掃除」節のとおり実装済み）
