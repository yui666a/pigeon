# メール送信機能 設計書

## 概要

Pigeonにメール送信機能を追加する。SMTP送信（新規作成・返信・全員に返信・転送）、送信済みメールのSentフォルダ保存、compose UIを実装する。

本体設計書 `2026-04-12-pigeon-design.md` の Phase 4「SMTP送信（新規・返信・転送）/ メール作成エディタ」に対応する。

### スコープ

| 含む | 含まない（将来） |
|------|----------------|
| プレーンテキストメールの送信 | リッチテキスト（TipTap）・HTML送信 |
| 新規作成 / 返信 / 全員に返信 / 転送 | 添付ファイルの送信 |
| 返信時のスレッディングヘッダー付与 | 下書き保存（Drafts同期） |
| 送信後のSentフォルダ保存とローカルDB取り込み | 送信予約・undo send |
| PLAIN / OAuth2 (XOAUTH2) 両認証 | |
| キーボードショートカット n / r / a / f | |

v1をプレーンテキストに限定する理由: 送信経路（SMTP・認証・スレッディング・Sent保存）の確立が本質であり、エディタのリッチ化は独立して後付けできる。受信メールの引用もbody_textを使えば劣化しない。

## アーキテクチャ

```
ComposeModal (React)
    │ invoke("send_mail", SendMailRequest)
    ▼
commands/send_commands.rs
    ├─ 1. 入力検証（宛先必須・メールアドレス形式）
    ├─ 2. reply_to_mail_id があれば DB から元メールを取得し
    │     In-Reply-To / References を構築
    ├─ 3. resolve_credentials で認証情報を解決（IMAP と共通化）
    ├─ 4. mail_sync/smtp_client.rs で SMTP 送信 (lettre)
    ├─ 5. Sentフォルダへ保存
    │     ├─ Google: 何もしない（GmailはSMTP送信時に自動でSentへ保存）
    │     └─ Other:  IMAP APPEND（ベストエフォート。失敗しても送信は成功扱い）
    └─ 6. ローカルDBに folder='Sent' で挿入（FTS5にも自動反映）
```

### モジュール構成

| ファイル | 責務 |
|---------|------|
| `mail_sync/smtp_client.rs` | lettreによるSMTP接続・送信。メッセージ構築（ヘッダー含む） |
| `commands/send_commands.rs` | `send_mail` command。検証→構築→送信→Sent保存のオーケストレーション |
| `commands/mail_commands.rs` | `resolve_imap_credentials` を `resolve_credentials` として共通利用（IMAP/SMTP両方が使う） |
| `src/components/compose/ComposeModal.tsx` | 作成画面（モーダル）。宛先/件名/本文 |
| `src/stores/composeStore.ts` | compose状態（開閉・モード・プリフィル・送信中） |
| `src/utils/composePrefill.ts` | 返信/転送時の宛先・件名・引用本文の組み立て（純関数） |

## データ設計

### SendMailRequest (Tauri command 入力)

```rust
#[derive(Deserialize)]
pub struct SendMailRequest {
    pub account_id: String,
    pub to: Vec<String>,          // 1件以上必須
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// 返信元メールのローカルID。In-Reply-To / References の構築に使う。
    /// 転送・新規では None
    pub reply_to_mail_id: Option<String>,
}
```

スレッディングヘッダーはフロントで組み立てず、Rust側で元メールのDBレコードから導出する（RFC 2822準拠の責務をバックエンドに集約）:

- `In-Reply-To` = 元メールの `message_id`
- `References` = 元メールの `references` + 元メールの `message_id`（連結、重複除去）

### Message-ID 生成

`<{uuid}@pigeon.local>` 形式で自機生成（lettreの自動生成に任せず、送信後にローカルDBへ保存する `message_id` と一致させる）。

### 送信後のローカル保存

`mails` テーブルに `folder='Sent'`, `uid=0`, `flags='\\Seen'` で挿入する。
uid=0 の理由: APPENDしたメールのUIDを取得するにはUIDPLUS拡張が必要で、対応しないサーバーもある。Sentフォルダは差分同期の対象外（現状INBOXのみ同期）のため、UID整合性の問題は生じない。将来Sentフォルダ同期を実装する際は message_id ベースの重複排除（既存のUNIQUE制約）で自然にマージされる。

## SMTP接続 (lettre)

```toml
lettre = { version = "0.11", default-features = false,
           features = ["tokio1", "tokio1-rustls-tls", "smtp-transport", "builder"] }
```

TLS方式はポートで自動判定する:

| ポート | 方式 |
|-------|------|
| 465 | Implicit TLS (`relay`) |
| 587 / その他 | STARTTLS (`starttls_relay`) |

認証:

| AuthType | lettre Mechanism | credential |
|----------|-----------------|-----------|
| Plain | `Mechanism::Plain` / `Login` | キーチェーンのパスワード |
| Oauth2 | `Mechanism::Xoauth2` | OAuthアクセストークン（IMAPと同じ `resolve_credentials` でリフレッシュ込みで解決） |

XOAUTH2のcredential文字列はlettreが `user=..\x01auth=Bearer ..\x01\x01` を組み立てるため、`Credentials::new(email, access_token)` を渡すのみ。

タイムアウト: 接続・送信とも30秒（`tokio::time::timeout`でラップ）。

## Sentフォルダ保存

- **Google**: SMTP送信するとGmailが自動的に「送信済み」へ保存するため、APPENDすると二重になる。何もしない。
- **Other**: 既存のIMAP接続コード（`imap_client::connect`）でセッションを張り、`APPEND "Sent" (\Seen)` を実行。フォルダ名は settings の `sent_folder`（デフォルト `"Sent"`）。失敗時は `eprintln!` ログのみで送信自体は成功として返す（送信は完了しており、ユーザーの操作としては成功のため）。

## UI設計

### ComposeModal

画面中央のモーダル。開くトリガー:

| トリガー | モード | プリフィル |
|---------|-------|-----------|
| Sidebar下部の「✉ 新規作成」ボタン / `n` | new | 空 |
| MailViewの「返信」ボタン / `r` | reply | To=元メールFrom、件名=`Re: `付与、本文=引用 |
| MailViewの「全員に返信」ボタン / `a` | replyAll | To=元From+元To(自分除く)、Cc=元Cc(自分除く)、他はreplyと同じ |
| MailViewの「転送」ボタン / `f` | forward | 件名=`Fwd: `付与、本文=元メール全体を引用ブロックで |

### プリフィル規則（composePrefill.ts の純関数）

- 件名: 既に `Re: `（大文字小文字問わず）で始まる場合は付け直さない。`Fwd: ` も同様
- 引用: `On {date}, {from} wrote:` 相当の日本語ヘッダー `{date} {from}:` + 各行 `> ` プレフィックス。引用元は `body_text`（無ければ空）
- replyAll の自分除外: アカウントのメールアドレスと大文字小文字無視で一致するものを To/Cc から除く

### 送信中・結果の扱い

- 送信中はSendボタンをスピナー付きdisabledに
- 成功: モーダルを閉じてトースト（既存の `errorStore` と対になる簡易通知。なければ `ErrorToast` の流用でsuccess表示を追加）
- 失敗: モーダルは開いたまま `errorStore.addError`（入力内容を失わせない）

### キーボードショートカット

`useKeyboardShortcuts` フック（App直下）。`input` / `textarea` / `contenteditable` にフォーカスがある場合、またはモーダルが開いている場合は無効。

- `n`: 新規作成
- `r` / `a` / `f`: 選択中メールに対して返信 / 全員に返信 / 転送（未選択時は何もしない）

## エラーハンドリング

- `AppError::Smtp(String)` を追加（thiserror）。Tauri境界では既存どおり文字列化
- OAuthトークン失効: `resolve_credentials` が既存の `AppError::ReauthRequired` を返す → フロントは同期と同じ再認証導線
- 宛先0件・不正アドレスは送信前に `AppError::Validation` で弾く（lettreの `Mailbox` パースを利用）

## テスト計画（TDD）

### Rust

- `smtp_client`: メッセージ構築の純関数部分（ヘッダー、In-Reply-To/References導出、Message-ID形式、宛先パース）をユニットテスト。実SMTP送信はモック境界の外とし統合テストでは行わない
- `send_commands`: バリデーション（宛先必須・不正アドレス）、reply時のReferences構築（DBフィクスチャ使用）、送信後のローカルDB挿入
- Sentフォルダ保存の分岐（Google→skip / Other→APPEND試行）はロジックを純関数に切り出してテスト

### React

- `composePrefill`: reply/replyAll/forward各モードのプリフィル（件名Re:重複、引用、 自分除外）
- `ComposeModal`: 表示・入力・送信invoke呼び出し・失敗時にモーダルが閉じないこと
- `useKeyboardShortcuts`: input フォーカス時に発火しないこと

## PR分割

1. **PR1 `feat/smtp-send-backend`**: lettre導入、smtp_client.rs、send_commands.rs、resolve_credentials共通化、Sent保存。本設計書もこのPRに含める
2. **PR2 `feat/compose-ui`**（PR1にスタック）: ComposeModal、composeStore、composePrefill、ショートカット、MailViewアクションボタン
