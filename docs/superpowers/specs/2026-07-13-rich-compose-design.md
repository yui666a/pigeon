# リッチテキスト送信 + 添付ファイル送信 設計書

## 概要

ComposeModal にリッチテキスト（TipTap）送信と添付ファイル送信を追加する。
バックログ（`docs/BACKLOG.md` 項目2）に対応する。送信経路（SMTP・認証・スレッディング・Sent保存）は
`2026-07-12-mail-send-design.md` で確立済みで、本件はそのメッセージ構築を multipart に拡張する。

本設計は下書き保存機能（`2026-07-12-draft-save-design.md` / PR #92）の上にスタックする。
composeStore の自動保存・DraftList を壊さずに拡張する。

### スコープ

| 含む | 含まない（将来） |
|------|----------------|
| リッチ/プレーンをメール単位で切り替えるトグル | 表・画像埋め込み・色/フォント等のリッチ装飾 |
| TipTap 最小構成（太字・斜体・リンク・箇条書き） | インライン画像（cid:）送信（受信側は項目5で別途） |
| `multipart/alternative`（text/plain + text/html）送信 | HTML の細かなサニタイズ方針（送信は自前生成HTMLのみ） |
| デフォルト送信形式の設定（localStorage） | 専用の設定画面統合（バックログ項目14） |
| 添付ファイル送信（`multipart/mixed`、合計25MB上限） | 添付の下書き保存・サーバー往復・進捗表示 |

## 採用方式

### メッセージ構造

lettre の MIME ツリーで組み立てる。本文と添付の有無で4通り:

| 本文 | 添付 | 構造 |
|------|------|------|
| プレーンのみ | なし | `text/plain`（従来どおり singlepart） |
| リッチ | なし | `multipart/alternative`（text/plain + text/html） |
| プレーンのみ | あり | `multipart/mixed`[ `text/plain`, 添付… ] |
| リッチ | あり | `multipart/mixed`[ `multipart/alternative`, 添付… ] |

RFC 上、`alternative` は「同一内容の代替表現」を並べ、対応クライアントが html を、
非対応クライアントが plain を表示する。plain は必ず含める（テキスト専用クライアント・アクセシビリティのため）。

### plain フォールバックの生成

リッチ本文は TipTap が生成した HTML。plain は **Rust 側で HTML から生成**する
（`html_to_plain` 純関数）。生成をバックエンドに集約する理由:

- メッセージ構築（RFC 準拠の責務）は既に `smtp_client.rs` に集約されている
- フロントは HTML 一つを送ればよく、plain との二重管理・不整合を防げる

`html_to_plain` の変換規則（最小・堅牢志向）:

- `<br>` → 改行、ブロック終了タグ（`</p>` `</div>` `</li>` `</h1..6>`）→ 改行
- `<li>` → 先頭に `- `
- それ以外のタグは除去、テキストノードは連結
- HTML エンティティ（`&amp; &lt; &gt; &quot; &#39; &nbsp;`）をデコード
- 連続する空行は最大2行に圧縮し、前後の空白を trim

厳密な HTML パーサは導入しない（送信対象は自前 TipTap 出力に限定されるため、
タグ境界ベースの走査で十分。外部由来 HTML の変換は本件のスコープ外）。

### 添付ファイル

- フロント: `@tauri-apps/plugin-dialog` の `open({ multiple: true })` でパスを取得し、
  `{ path, name, size }` のリストを composeStore に保持する
- 送信時は **パスの配列**を `SendMailRequest.attachments` で渡す。
  ファイルの実体（バイト列）は Rust が `std::fs::read` で読む
  （大きいバイナリを base64 で IPC 経由に載せない。既存の添付保存 `save_attachment` と同じくパス受け渡し方式）
- Content-Type は拡張子から素朴に推定（不明は `application/octet-stream`）。
  ファイル名は `Content-Disposition: attachment; filename=...` に載る（lettre `Attachment::new`）

### サイズ上限

合計 **25MB**（`MAX_TOTAL_ATTACHMENT_BYTES = 25 * 1024 * 1024`、Gmail 準拠）。
Rust 側で全添付を読み込んだ後に合計を検証し、超過は `AppError::Validation` で送信前に弾く。
フロントでも選択時点の合計サイズを表示し、超過を赤字で警告する（送信ボタンは Rust の検証に委ねる二重防御）。

### デフォルト送信形式

localStorage キー `pigeon.composeFormat`（値 `"rich"` | `"plain"`、デフォルト `"plain"`）。
既存の通知トグル（`pigeon.notifyNewMail`）と同じ方式。ComposeModal を開くたびにこの既定値で
初期化し、トグル横の「既定にする」操作で現在の選択を保存する。
**設定画面への統合はバックログ項目14の対象で、本件では ComposeModal 内に閉じる。**

## データ設計

### SendMailRequest 拡張（Rust / TypeScript）

```rust
pub struct SendMailRequest {
    // 既存フィールドは変更しない
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub reply_to_mail_id: Option<String>,
    /// リッチ本文の HTML。None ならプレーンのみ送信（後方互換）
    pub body_html: Option<String>,
    /// 添付ファイルの絶対パス。空なら添付なし
    pub attachments: Vec<String>,
}
```

`body_text` はリッチ時も送る必要はなく（Rust が html から生成）、フロントは
リッチ時 `body_text=""`・`body_html=Some(html)`、プレーン時 `body_html=None` を送る。
既存の送信フロー・`build_sent_record` の uid ロジックには手を入れない
（別途 Sent 同期の修正が並行しているため）。

### OutgoingMail 拡張

```rust
pub struct OutgoingMail {
    // 既存 + 追加:
    pub body_html: Option<String>,
    pub attachments: Vec<OutgoingAttachment>,
}
pub struct OutgoingAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}
```

## テスト計画（TDD）

### Rust

- `html_to_plain`: br/ブロック改行、li の `- `、エンティティデコード、タグ除去、空行圧縮
- `build_message`:
  - body_html なし・添付なし → 従来どおり（`Content-Type: text/plain`）
  - body_html あり → `multipart/alternative` に text/plain と text/html を含む
  - 添付あり → `multipart/mixed`、filename がヘッダーに載る
  - リッチ + 添付 → mixed 内に alternative
- 添付サイズ上限: 合計が上限超で `Validation` エラー（`validate_attachment_size` 純関数）
- 拡張子→Content-Type 推定（`guess_content_type`）

### React

- `composeFormat` util: get/set（デフォルト plain、localStorage 読み書き）
- `ComposeModal`: トグルでリッチ/プレーン切替、既定化操作で localStorage 更新、
  添付追加/削除でリストが変わること、送信 invoke に body_html/attachments が載ること
- リッチ時に送信すると `body_html` が非 null で `send_mail` が呼ばれること

## v1 の制限（既知）

- **下書き保存はプレーンに落として保存**する。`drafts` テーブルのスキーマは変えない
  （`body_text` のみ）。リッチ本文は保存時に `html_to_plain` 相当でプレーン化して保存し、
  復元時はプレーンモードで開く。**添付は下書きに保存しない**（パスは端末依存で永続化に不向き）
- 返信/転送の引用（`composePrefill.ts`）は**プレーンテキストのまま**。リッチモードで返信した場合、
  引用はテキストとして本文（HTML化前の初期値）に含まれる。引用の HTML 整形はしない
- Content-Type 推定は拡張子ベースの素朴な実装。MIME sniffing はしない
- 送信の HTML はサニタイズしない（自前 TipTap 出力のみが対象で、外部 HTML を送らないため）
- 設定は ComposeModal 内に閉じる（専用設定画面はバックログ項目14）
</content>
</invoke>
