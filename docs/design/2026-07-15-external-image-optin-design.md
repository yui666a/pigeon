# 外部画像の表示オプトイン設計（BACKLOG C9）

- 作成: 2026-07-15
- ステータス: 実装中
- 関連: `docs/plans/2026-07-15-security-hardening-4-items.md` Part 2 / セキュリティ監査 C9

## 背景と目的

外部サーバー上の画像（`<img src="https://...">`）は現在2層で完全に遮断している。

1. CSP `img-src 'self' data: blob:` が Webview からの読み込みを禁止
2. `sanitizeMailHtml` が `data:image/` と `cid:` 以外の `src` 属性を除去（2026-07-15 #155）

これはトラッキングピクセル対策（開封時刻・IPアドレスの送信者への通知防止）として正しい既定だが、画像入りのメールを読みたいユーザーの正当な需要がある。本設計は「既定は遮断のまま、ユーザーの明示操作でのみ表示する」オプトイン経路を追加する。

**前提: オプトインしても、取得した瞬間に開封が送信者へ通知される事実は変わらない。** 本機能の意味は、その通知を「知らないうちに起きる」から「ユーザーが明示的に選んだ結果」に変えることにある。

## 設計

### 全体フロー

```
MailBody 描画
  → extractExternalImageUrls(resolvedHtml) で外部画像URLを列挙
  → 1件以上あれば「画像を表示 (N)」ボタンを表示（本文は画像なしで先に表示）
  → クリック → invoke("fetch_external_images", { urls })
  → Rust: reqwest で取得 → 検証 → data URI 化して返却
  → replaceExternalImageUrls で元URLを data URI に置換
  → sanitizeMailHtml → srcdoc iframe で描画（既存の描画経路に合流）
```

### CSP は緩めない（不変条件）

`img-src` に外部オリジンを足さない。画像は Rust 側で取得して `data:image/...` に変換するため、既存 CSP・サニタイザの許可リスト（`data:image/` プレフィックス）をそのまま通る。Webview から外部への直接リクエストは今後も発生しない。

### フロントエンド

- `src/utils/externalImages.ts`
  - `extractExternalImageUrls(html: string): string[]` — DOMParser で `img[src]` を走査し、`http(s)://` で始まる src を重複除去して返す。上限 20 件（超過分は無視）。プロトコル相対 URL（`//...`）は対象外（従来どおり除去されたまま）
  - `replaceExternalImageUrls(html, images: { url, data_uri }[]): string` — `replaceCidReferences` と同型。data URI が `data:image/` で始まるもののみ差し替える（多層防御）
- `MailBody`: 外部画像が検出されたメールにのみボタンを表示。取得結果はコンポーネント state に保持し、**永続化しない**（メールを開き直したら再び遮断状態に戻る）
- 取得中はボタンを無効化。失敗時は既存のエラー表示経路（toast）で通知

### バックエンド（Rust）

`src-tauri/src/commands/remote_image_commands.rs`

```rust
#[tauri::command]
pub async fn fetch_external_images(urls: Vec<String>) -> Result<Vec<FetchedImage>, AppError>
// FetchedImage { url: String, data_uri: String }
```

取得ポリシー（すべて検証は純関数に切り出して単体テストする）:

| 項目 | 値 | 理由 |
|---|---|---|
| スキーム | http / https のみ | file:/カスタムスキームの読み出し禁止 |
| ホスト | IPリテラルの private / loopback / link-local と `localhost` を拒否 | メール由来URLによる内部ネットワークのプローブ防止 |
| リダイレクト | 最大5回。各ホップを同じ検証に通す | 検証済みURL→内部アドレスへの迂回防止 |
| 枚数 | 最大20 | 巨大メールでの資源枯渇防止 |
| サイズ | 1枚 5MB（チャンク読みで超過時中断） | メモリ枯渇防止 |
| タイムアウト | 10秒/枚 | ハング防止 |
| Content-Type | `image/*`（英数と `.+-` のみ）以外は拒否 | data URI への非画像型・ヘッダ注入の防止 |
| Cookie / 認証情報 | 送らない（素の GET） | トラッキング面の最小化 |

一部失敗は全体を失敗させず、取得できた画像だけ返す（`FetchedImage` に含まれない URL は遮断されたまま表示される）。

### 制限（設計時点で把握している残余リスク）

- **DNSリバインディング**: ホスト名が private IP に解決されるケースはブロックしない（IPリテラルのみ検査）。ローカルデスクトップアプリであり、攻撃者が得るのは「ユーザー自身のLAN内HTTPレスポンスの画像としての表示」に限られるため v1 では許容。対策する場合は resolve 後の IP 検査を reqwest の DNS レイヤーに差し込む
- 開封通知としての性質はオプトインの本質であり残る（上記前提）

## 却下した代替案

- **CSP img-src へ https: を追加して直接表示** — Webview から外部へ直接リクエストが飛び、CSP の「外部と話さない」不変条件が壊れる。取得制御（サイズ・型・ホスト検証）も効かない。却下
- **表示許可の永続化（メール単位/送信者単位）** — 状態管理・DB スキーマ・設定 UI を伴う。まず非永続の最小形で価値を確認し、送信者単位の「常に表示」は将来課題として BACKLOG に残す
- **Rust 側での HTML 書き換え** — 置換ロジックが cid 置換（フロント実装）と二重管理になる。既存の cid 置換と同じ場所・同じ形に揃える

## 将来課題

- 送信者単位の「この差出人の画像を常に表示」（永続化・設定画面統合 #14 とセット）
- DNSリバインディング対策（resolve 後 IP 検査)
- 取得画像のキャッシュ（現状は表示のたびに再取得）
