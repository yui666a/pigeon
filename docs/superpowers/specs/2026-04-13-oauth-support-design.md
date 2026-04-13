# OAuth 2.0 対応（Gmail）設計書

## 概要

Gmail に OAuth 2.0 で接続できるようにする。カスタムURLスキーム（`com.haiso.pigeon://oauth/callback`）でブラウザからの認可コードを受け取り、Stronghold にトークンを保存する。将来の iOS / 他プロバイダ対応を見据えた設計。

### スコープ

- Gmail の OAuth 2.0 IMAP 接続（XOAUTH2）
- PLAIN 認証のパスワードも Stronghold に移行（現在は毎回引数で渡している）
- SMTP は Phase 4 のスコープのため今回は対象外

### フェーズ計画との関係

メイン設計書では OAuth 2.0 は Phase 5 に位置づけられているが、Gmail 接続を早期に実現するため前倒しで実装する。メイン設計書のフェーズ計画を更新すること。

### 今回スコープ外

- Yahoo! Japan メール（IMAP XOAUTH2 対応が不明なため。別途検証後に対応）
- SMTP の OAuth 認証（Phase 4 で SMTP 送信と合わせて実装）

## Gmail OAuth 設定

| 項目 | 値 |
|------|-----|
| IMAP | imap.gmail.com:993 (TLS) |
| SMTP | smtp.gmail.com:587 (STARTTLS)（Phase 4 で使用） |
| 認可URL | https://accounts.google.com/o/oauth2/v2/auth |
| トークンURL | https://oauth2.googleapis.com/token |
| スコープ | `https://mail.google.com/` `openid` `email` |
| IMAP認証方式 | XOAUTH2 |

スコープに `openid email` を含めることで、トークンレスポンスの ID Token からメールアドレスを取得できる。

### 開発時の運用

- Google Cloud Console でクライアント種別「**デスクトップアプリ**」として OAuth クライアントを登録
- OAuth 同意画面を「テストモード」で運用（指定したテストユーザーのみ利用可能）
- 公開時に Google の審査に申請。コード変更は不要

## OAuth クライアント登録

Google Cloud Console (https://console.cloud.google.com/) で設定:

- アプリの種類: **デスクトップアプリ**
- リダイレクト URI: `com.haiso.pigeon://oauth/callback`

クライアント ID / シークレットはビルド時に環境変数から読み込む:

```
PIGEON_GOOGLE_CLIENT_ID=xxx
PIGEON_GOOGLE_CLIENT_SECRET=xxx
```

### クライアントシークレットについて

Google の「デスクトップアプリ」クライアントではシークレットは**公開情報として扱われる**（Google のドキュメントに明記）。リバースエンジニアリングで抽出可能であることは想定内であり、PKCE によってセキュリティを担保する。シークレットはバイナリに埋め込む。

## 認証フロー

```
 1. ユーザーがプロバイダ選択画面で「Google」を選択
 2. 「Google でログイン」ボタンをクリック
 3. Rust バックエンドが account_id を事前に採番（UUID）
 4. PKCE code_verifier + code_challenge を生成
 5. state パラメータを生成し、(state, account_id, code_verifier) をメモリに保持
 6. 認可URLを生成（access_type=offline, prompt=consent を含む）
 7. OS デフォルトブラウザで認可URLを開く（tauri-plugin-opener）
 8. ユーザーがブラウザでログイン・許可
 9. ブラウザが com.haiso.pigeon://oauth/callback?code=xxx&state=yyy にリダイレクト
10. tauri-plugin-deep-link がリダイレクトを受け取る
11. state パラメータを検証（CSRF対策）、対応する code_verifier を取得
12. 認可コードをトークンエンドポイントに送信
    → アクセストークン + リフレッシュトークン + ID Token を取得
13. ID Token をデコードして email claim からメールアドレスを取得
14. トークンを Stronghold に保存（キー: oauth_{account_id}）
15. accounts テーブルにアカウント情報を保存（provider = 'google'）
16. UI にアカウント追加完了を通知
```

### リフレッシュトークン取得の条件

Google で `refresh_token` を取得するには、認可リクエストに以下のパラメータが必須:

- `access_type=offline` — リフレッシュトークンの発行を要求
- `prompt=consent` — 毎回同意画面を表示（2回目以降も refresh_token を確実に取得するため）

### PKCE (Proof Key for Code Exchange)

- `code_verifier`: ランダム文字列（43〜128文字、英数字 + `-._~`）
- `code_challenge`: code_verifier の SHA-256 ハッシュを Base64URL エンコード
- `code_challenge_method`: S256
- `code_verifier` の TTL: 10分。タイムアウトしたらメモリから破棄し、ユーザーにやり直しを促す

### トークンリフレッシュ

- IMAP 接続前にアクセストークンの `expires_at` を確認
- **有効期限の 5 分前**にリフレッシュを実行（バッファ）
- リフレッシュ成功: 新しいアクセストークンを Stronghold に上書き保存
- リフレッシュ失敗（401）: リフレッシュトークンが無効化された。ユーザーに再認証を促す通知を出す

## IMAP XOAUTH2 認証

OAuth 2.0 で取得したアクセストークンを使って IMAP に認証する:

```
AUTHENTICATE XOAUTH2 <base64_encoded_auth_string>
```

auth_string のフォーマット:
```
user=<email>\x01auth=Bearer <access_token>\x01\x01
```

`\x01` は SOH (0x01) バイト。auth_string 全体を Base64 エンコードして送信する。

## データモデルの変更

### accounts テーブル

```sql
-- マイグレーション V2
ALTER TABLE accounts ADD COLUMN provider TEXT NOT NULL DEFAULT 'other'
    CHECK(provider IN ('google', 'other'));
```

既存のアカウントは `provider = 'other'` になる。将来 Yahoo! Japan 等を追加する場合は CHECK 制約を更新する。

### Rust モデル

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProvider {
    Google,
    Other,
}

impl AccountProvider {
    pub fn supports_oauth(&self) -> bool {
        matches!(self, Self::Google)
    }
}
```

`OAuthProvider` は別途定義しない。`AccountProvider` で OAuth 対応かどうかを判別する。

### Stronghold に保存するデータ

OAuth トークン（キー: `oauth_{account_id}`）:

```json
{
    "access_token": "ya29.xxx",
    "refresh_token": "1//xxx",
    "expires_at": 1713000000,
    "email": "user@gmail.com"
}
```

PLAIN 認証のパスワード（キー: `password_{account_id}`）:

```json
{
    "password": "xxx"
}
```

### フロー順序とデータ整合性

account_id はフローのステップ 3 で事前に UUID 採番する。これにより:

- トークン保存（ステップ 14）で `oauth_{account_id}` キーが使える
- DB 保存（ステップ 15）が失敗した場合、Stronghold のエントリを削除する補償処理を実行
- OAuth フロー全体が失敗した場合（タイムアウト等）、code_verifier とともに account_id も破棄

## sync_account コマンドの変更

現在のシグネチャ:

```rust
pub async fn sync_account(
    state: State<'_, DbState>,
    account_id: String,
    imap_host: String,
    imap_port: u16,
    username: String,
    password: String,
) -> Result<u32, String>
```

変更後: `account_id` のみを受け取り、接続情報と認証情報は内部で解決する:

```rust
pub async fn sync_account(
    state: State<'_, DbState>,
    stronghold: State<'_, StrongholdState>,
    account_id: String,
) -> Result<u32, String>
```

内部処理:
1. `account_id` で accounts テーブルからアカウント情報を取得
2. `provider` で分岐:
   - `google`: Stronghold から OAuth トークンを取得 → 必要ならリフレッシュ → XOAUTH2 で IMAP 接続
   - `other`: Stronghold からパスワードを取得 → PLAIN で IMAP 接続

## Rust モジュール構成

```
src-tauri/src/
├── mail_sync/
│   ├── imap_client.rs      # connect を auth_type で分岐（PLAIN / XOAUTH2）
│   ├── mime_parser.rs      # 変更なし
│   ├── mod.rs
│   └── oauth.rs            # 新規: OAuth フロー全体
├── commands/
│   ├── account_commands.rs # provider 対応、Stronghold パスワード保存
│   ├── auth_commands.rs    # 新規: OAuth コマンド
│   ├── mail_commands.rs    # sync_account シグネチャ変更
│   └── mod.rs
├── models/
│   ├── account.rs          # AccountProvider 追加
│   └── mail.rs             # 変更なし
└── db/
    ├── accounts.rs         # provider カラム対応
    └── migrations.rs       # V2 マイグレーション追加
```

### 新規ファイル: mail_sync/oauth.rs

責務:
- Gmail の OAuth 設定定数（認可URL、トークンURL、スコープ、クライアント情報）
- 認可URL生成（PKCE + state + access_type=offline + prompt=consent）
- 認可コード → トークン交換（アクセストークン + リフレッシュトークン + ID Token）
- ID Token デコード（JWT のペイロードから email claim を取得。署名検証は不要 — HTTPS 直接通信のため）
- トークンリフレッシュ
- XOAUTH2 auth string の生成

### 新規ファイル: commands/auth_commands.rs

Tauri commands:
- `start_oauth(provider)` → 認可URLを返す + ブラウザで開く
- `handle_oauth_callback(url)` → コールバックURL処理、トークン取得、アカウント保存

## フロントエンドの変更

### AccountForm → プロバイダ選択 + 手動入力

プロバイダ選択画面:

```
┌─────────────────────────────┐
│  アカウントを追加            │
│                              │
│  ┌─────────────────────────┐ │
│  │  G  Google でログイン    │ │
│  └─────────────────────────┘ │
│  ┌─────────────────────────┐ │
│  │     その他（手動設定）    │ │
│  └─────────────────────────┘ │
│                              │
│  [キャンセル]                │
└─────────────────────────────┘
```

- 「Google」: 「Google でログイン」ボタンのみ。クリックでブラウザが開く
- 「その他」: 従来の手動入力フォーム（IMAP/SMTP/パスワード）

### OAuth フロー中の UI 状態

```
「Google でログイン」クリック
  → ボタンがローディング表示に変わる
  → ブラウザが開く
  → 画面に「ブラウザで認証中です... キャンセル」を表示
  → 10分タイムアウト → 「認証がタイムアウトしました。もう一度お試しください。」
  → コールバック受信 → 「アカウントを設定中...」
  → 完了 → アカウント一覧に追加、フォームを閉じる
  → 失敗 → エラーメッセージ表示
```

### accountStore.ts

- `startOAuth(provider)` — OAuth フロー開始
- `handleOAuthCallback(url)` — コールバック処理
- `oauthStatus: 'idle' | 'waiting' | 'exchanging' | 'error'` — OAuth 状態

### mailStore.ts

- `syncAccount` の引数を `(accountId: string)` のみに変更（接続情報はバックエンドで解決）

## Tauri プラグイン

追加が必要なプラグイン:

| プラグイン | 用途 |
|-----------|------|
| `tauri-plugin-deep-link` | カスタムURLスキーム `com.haiso.pigeon://` の登録とコールバック受信 |
| `tauri-plugin-stronghold` | OAuth トークン / パスワードのセキュア保存 |

### Deep Link スキーム

`com.haiso.pigeon://oauth/callback` を使用する。リバースドメイン形式にすることで他アプリとの衝突リスクを軽減する。

- macOS: `Info.plist` の `CFBundleURLSchemes` に登録（Tauri が自動設定）
- iOS: 同上（将来対応時）
- Windows: レジストリ登録（Tauri が自動設定）

## エラーハンドリング

| エラーケース | 対応 |
|-------------|------|
| ネットワーク障害（トークン交換時） | 最大3回リトライ（1秒、3秒、5秒間隔）。失敗時はエラー表示 |
| 認可コードのタイムアウト | code_verifier の TTL 10分。タイムアウト後はメモリから破棄し「認証がタイムアウトしました」表示 |
| state 不一致（CSRF） | コールバックを拒否し「認証に失敗しました。もう一度お試しください」表示 |
| deep-link 未受信（ブラウザで放置） | 10分タイムアウトでフロー終了。「キャンセル」ボタンで即時終了も可能 |
| 同じメールアドレスのアカウントが既に存在 | トークン取得後、email で重複チェック。既存アカウントのトークンを更新するか確認ダイアログを表示 |
| DB 保存失敗（トークン保存後） | Stronghold のエントリを削除する補償処理を実行 |
| リフレッシュトークン無効化 | ユーザーに「再ログインが必要です」通知。アカウント一覧に警告マーク |

## セキュリティ

- クライアントシークレットはバイナリに埋め込む。Google「デスクトップアプリ」クライアントではシークレットは公開情報扱い（リバースエンジニアリングで抽出可能なことは想定内）。PKCE でセキュリティを担保
- state パラメータで CSRF を防止
- PKCE (S256) でコード横取り攻撃を防止
- トークンは Stronghold に暗号化保存。SQLite には保存しない
- アクセストークンのメモリ上の保持は最小限にする（使用後に即破棄はしないが、グローバル変数には持たない）

## テスト方針

### ユニットテスト（自動）

- `oauth.rs`: 認可URL生成、PKCE code_verifier/code_challenge 生成、XOAUTH2 auth string 生成、ID Token デコード
- `imap_client.rs`: 認証方式の分岐ロジック（PLAIN / XOAUTH2）
- `migrations.rs`: V2 マイグレーション（provider カラム追加、既存データのデフォルト値）
- `accounts.rs`: provider 付き CRUD
- フロントエンド: プロバイダ選択画面のレンダリング、OAuth 状態遷移の表示

### 手動テスト

- Google OAuth フロー全体（認可 → トークン取得 → IMAP 接続 → メール取得）
- トークンリフレッシュ（アクセストークン期限切れ後の再接続）
- エラーケース（ネットワーク切断、認証キャンセル、タイムアウト）
