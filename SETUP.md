# Pigeon セットアップガイド

## 1. 必要要件

| ツール | バージョン | 用途 |
|--------|-----------|------|
| [mise](https://mise.jdx.dev/) | 最新 | Rust / Node.js / pnpm のバージョン管理（推奨） |
| Rust | 1.94+ | バックエンド（miseで導入可） |
| Node.js | 22+ | フロントエンド（miseで導入可） |
| pnpm | 10+ | パッケージマネージャ（miseで導入可） |
| [Ollama](https://ollama.com/) | 最新 | AI分類用ローカルLLM（デフォルトプロバイダ） |

加えて、Tauri 2 の[システム依存関係](https://v2.tauri.app/start/prerequisites/)が必要です（macOSはXcode Command Line Tools、Linuxはwebkitgtkほか）。

## 2. インストール

```bash
git clone https://github.com/yui666a/pigeon.git
cd pigeon

# ツールチェーンを一括インストール（mise.toml のバージョンで固定）
mise install

# フロントエンド依存関係
pnpm install
```

## 3. Ollama の準備（AI分類を使う場合）

```bash
# Ollama をインストール後、デフォルトモデルを取得
ollama pull llama3.1:8b

# Ollama が http://localhost:11434 で起動していることを確認
ollama serve
```

- エンドポイント・モデルはアプリ内の設定（⚙ → LLM設定）で変更できます
- クラウドLLM（Claude API / Vertex AI）を使う場合もLLM設定から切り替えます。**クラウド選択時はメールの一部（件名・送信者・本文冒頭）が外部APIへ送信される**ため、アプリが警告を表示します

## 4. Gmail を使う場合（OAuth 2.0）

Gmailアカウントの追加には、自分のGoogle CloudプロジェクトでOAuthクライアントを作成する必要があります。

1. [Google Cloud Console の認証情報](https://console.cloud.google.com/auth/clients)で「OAuth 2.0 クライアント ID」を作成（アプリケーションの種類: **デスクトップアプリ**）
2. Gmail API を有効化し、OAuth同意画面でスコープ `https://mail.google.com/` を設定
3. `.env` を作成してクライアント情報を記入:

```bash
cp .env.sample .env
chmod 600 .env  # 所有者のみ読書き可にする
```

```dotenv
PIGEON_GOOGLE_CLIENT_ID_DESKTOP=xxxx.apps.googleusercontent.com
PIGEON_GOOGLE_CLIENT_SECRET_DESKTOP=GOCSPX-xxxx
```

> `.env` は各自ローカルで作成し、コミット禁止です（`.gitignore` 済み）。トークンはOSキーチェーンに保存され、SQLiteには平文保存されません。
>
> デスクトップアプリの OAuth クライアントシークレットは公開クライアント扱いで厳密な秘密ではない（本来の防御は PKCE）が、漏洩するとレート悪用・なりすまし同意画面のリスクがあるため秘密として扱う。将来的にはシークレットレス（PKCE のみ）の構成への移行を検討する。

## 5. 起動と初期設定

```bash
pnpm tauri dev      # 開発モード
# または
pnpm tauri build    # リリースビルド（バンドルを生成）
```

初回起動後:

1. サイドバーの「**+ 追加**」からアカウントを追加
   - **Google**: 「Googleでログイン」→ ブラウザで認可（deep linkでアプリに戻ります）
   - **その他のIMAP/SMTP**: ホスト・ポート・パスワードを手動入力（IMAP: 993 / SMTP: 587 or 465）
2. 追加すると初回同期が始まります（デフォルトで直近 **5,000件** をバッチ取得。進捗はサイドバー下部に表示され、途中で閉じても次回続きから再開）
3. 未分類メールが溜まったら「**分類する**」ボタンでAI分類を実行。提案を承認/修正しながら案件が育ちます

## 6. データの保存場所

| データ | 場所 |
|--------|------|
| メール・案件DB | `~/Library/Application Support/Pigeon/pigeon.db`（macOS） |
| 添付ファイルキャッシュ | 同ディレクトリ `attachments/` |
| パスワード・OAuthトークン・APIキー | 同ディレクトリの暗号化ストア（Stronghold） |

## 7. テスト・リント

```bash
pnpm test                                   # フロントエンド (Vitest)
cd src-tauri && cargo test                  # Rust
cd src-tauri && cargo clippy -- -D warnings # リント
cd src-tauri && cargo fmt -- --check        # フォーマット確認
```

## 8. トラブルシューティング

| 症状 | 対処 |
|------|------|
| Gmailで「Reauth required」 | トークン失効。アカウント欄の再認証ボタンからOAuthをやり直す |
| AI分類が失敗する | `ollama serve` が起動しているか、モデル（`ollama list`）が存在するか確認。LLM設定の「接続テスト」も利用可 |
| 新着が自動で届かない | IMAP IDLE非対応サーバーでは15分間隔のポーリングにフォールバックします。手動同期はアカウントを選択し直すか再起動 |
| デスクトップ通知が出ない | OSの通知許可を確認。サイドバー下部のトグルがONか確認 |
| `pnpm tauri dev` がビルドエラー | Tauriのシステム依存関係（webkit等）を確認。`mise install` 後にシェルを再起動 |
