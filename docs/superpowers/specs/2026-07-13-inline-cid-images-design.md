# インライン画像（cid:）の本文内表示 設計

日付: 2026-07-13
ステータス: 実装

## 目的

HTMLメール本文中の `<img src="cid:...">` を実際の画像として表示する。
現状 `extract_attachments` は Content-ID を無視しており、添付一覧には出るが
本文中では画像が読み込めず壊れたアイコンになる（`docs/superpowers/specs/2026-07-12-attachment-download-design.md`
の将来課題）。

## 方針: 既存のオンデマンド取得＋キャッシュ経路を流用

添付ダウンロード機構（`2026-07-12-attachment-download-design.md`）と同じ
「IMAPから元メールを取得→ mail-parser で抽出 → ローカルキャッシュ」の流れを
そのまま使う。新規に増やすのは以下の2点のみ:

1. `attachments` テーブルに `content_id` カラムを追加し、`extract_attachments` が
   Content-ID ヘッダを認識してセットする
2. `content_id` を持つ添付だけを取り出して `content_id → data URI` の対応表を返す
   新コマンド `get_inline_images`

添付一覧（`list_attachments` / `AttachmentList.tsx`）とキャッシュ実体は完全に共有する。
cid画像も「添付」の一種であり、二重にダウンロード・キャッシュする理由がないため。

## データモデル

`migrate_v13` で `attachments` に `content_id` カラムを追加する
（v9〜v12 は並行開発中の他エージェントが使用するため空けている）:

```sql
ALTER TABLE attachments ADD COLUMN content_id TEXT;
CREATE INDEX IF NOT EXISTS idx_attachments_content_id ON attachments(mail_id, content_id);
```

`content_id` は Content-ID ヘッダの値から `<` `>` を除去したもの。
`Content-Disposition: inline` かどうかは判定に使わない
（Content-ID を持つパートは cid参照の対象になり得るため、両方を許可する
メーラーが多い。cid未参照でも実害はなく、通常の添付として一覧にも出る）。

## バックエンド

### 添付抽出（mail_sync/mime_parser.rs）

`ExtractedAttachment` に `content_id: Option<String>` を追加し、
`extract_attachments` が `part.content_id()` を読んで `<...>` を剥がしてセットする。

### DB（db/attachments.rs）

`insert_attachment` に `content_id: Option<&str>` 引数を追加。
`get_by_mail_id` / `get_by_id` の SELECT に `content_id` を追加し `Attachment` に含める。

### 新規コマンド（commands/inline_image_commands.rs）

`attachment_commands.rs` の変更は最小限（`cache_attachments` へ `content_id` を渡す配線のみ）
にとどめ、新規ファイルに切り出す。

- `get_inline_images(mail_id) -> Vec<InlineImage>`
  - `InlineImage { content_id: String, data_uri: String }`
  - `attachment_commands::load_cached_attachments` / IMAP取得 + `cache_attachments` を
    そのまま呼び出し、キャッシュヒット・ミスの経路を共有する
  - 返ってきた `Attachment` 一覧から `content_id.is_some()` のものだけ抽出し、
    キャッシュファイルを読んで base64エンコードし `data:{mime_type};base64,{...}` を組み立てる
  - `has_attachments = false` や添付なしの場合は空Vec

### invoke_handler 登録

`lib.rs` の `invoke_handler` へ `save_attachment` の直後に
`commands::inline_image_commands::get_inline_images` を追加する。

## フロントエンド

### cid → data URI 置換（src/utils/inlineImages.ts）

`replaceCidReferences(html: string, images: InlineImage[]): string` という純関数を追加する。
`<img src="cid:xxx">` の `cid:` スキームの `src` 属性のみを対象に、対応する `data_uri` があれば
置換する。対応がなければ変更しない（壊れたアイコン表示のまま＝安全側）。

DOM操作（`DOMParser` でパースして `img[src^="cid:"]` を置換）で実装し、正規表現による
HTML書き換えは避ける（属性値のエスケープ崩れを防ぐため）。

### MailBody.tsx

1. `mail.body_html` に `cid:` 参照があるかを検出したら `get_inline_images(mail.id)` を invoke
2. 取得完了まで本体は元のHTML（cid未解決のまま）を先に表示し、取得後に置換後HTMLへ差し替える
   （表示ブロックしない。プレースホルダは「壊れた画像アイコン」がブラウザ標準で出るのでそれに委ねる）
3. 置換後のHTMLを `DOMPurify.sanitize` に通す。サニタイズ設定は現状の `mail.body_html` 用と共通のまま
   （`data:` scheme の `img src` は DOMPurify のデフォルトで許可されている。外部URL画像の
   自動読み込みポリシーは変更しない = `<img src="https://...">` は元々許可されており今回変更なし）

## セキュリティ

- `cid:` → `data:` 置換は `content_id` がキャッシュから解決できたものだけに限定する。
  解決できない cid参照はそのまま残り、ブラウザが読み込みに失敗するだけ（外部リクエストは発生しない）
- 置換対象は `src` 属性が `cid:` スキームで始まる `img` 要素のみ。他の属性・他のスキームは触らない
- data URI 化した画像データは添付キャッシュと同じくローカルのみで完結し、LLMへは送信しない

## v1の制限（スコープ外）

- `background` 属性や CSS `url(cid:...)` など `img src` 以外の cid参照は対象外
- 添付一覧に "inline" 種別のバッジ表示はしない（`content_id` の有無で内部判定するのみ）
- インライン画像のキャッシュ容量上限・削除ポリシーは添付キャッシュと同様に未対応（既存の将来課題を継承）
