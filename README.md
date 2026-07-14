# 🕊 Pigeon

**AIがメールを案件ごとに自動グルーピングするデスクトップメールクライアント**

従来のフォルダベースの整理ではなく、ローカルLLM（Ollama）がメールを解析して「**案件 > スレッド > メール**」の階層で自動整理します。見積もり依頼から納品まで、1つの案件に関するやり取りが1か所にまとまります。

| | |
|---|---|
| フレームワーク | Tauri 2（Rust + React 19 / TypeScript） |
| 対応サーバー | Gmail（OAuth 2.0）/ 任意のIMAP/SMTPサーバー |
| AI分類 | Ollama（ローカル・デフォルト）/ Claude API / Vertex AI（オプション） |
| データ保存 | ローカルSQLite（FTS5全文検索）+ OSキーチェーン |

## ✨ 主な機能

- 🤖 **AI自動分類** — 新着メールを既存案件へ自動振り分け。新しい案件の作成も提案。手動修正から学習して精度が向上
- 🧵 **スレッド表示** — RFC 2822準拠の返信チェーン構築（件名フォールバック付き）
- 📤 **送信** — 新規作成 / 返信 / 全員に返信 / 転送。スレッディングヘッダー自動付与
- ⚡ **リアルタイム同期** — IMAP IDLEによるプッシュ受信 + デスクトップ通知
- 🔍 **全文検索** — SQLite FTS5による案件横断検索
- 🗑 **メール操作** — 既読管理・削除（ゴミ箱へ移動）・アーカイブ・添付ファイル保存
- 🔒 **プライバシー第一** — 認証情報はOSキーチェーン保管。デフォルトのOllamaならメール内容がマシンの外に出ない

全機能の詳細は **[FEATURES.md](FEATURES.md)** を参照。

## 🚀 クイックスタート

```bash
git clone https://github.com/yui666a/pigeon.git
cd pigeon
mise install        # Rust / Node.js / pnpm を一括インストール
pnpm install
cp .env.sample .env # Gmailを使う場合はOAuthクライアント情報を記入（SETUP.md参照）
pnpm tauri dev
```

詳細な手順（Ollamaの準備、Google OAuthクライアントの作成、トラブルシューティング）は **[SETUP.md](SETUP.md)** を参照。

## 🧪 開発

```bash
pnpm test                                  # フロントエンドテスト (Vitest)
cd src-tauri && cargo test                 # Rustテスト
cd src-tauri && cargo clippy -- -D warnings
pnpm tauri build                           # リリースビルド
```

開発フロー・Git戦略・コーディング規約は [CONTRIBUTING.md](CONTRIBUTING.md) と [agent.md](agent.md) を参照。

## 📁 プロジェクト構成

```
pigeon/
├── src/                    # React フロントエンド
│   ├── components/         # UI（3ペイン: 案件ツリー / スレッド一覧 / メール本文）
│   ├── stores/             # Zustand ストア
│   ├── hooks/              # カスタムフック（ショートカット等）
│   └── types/              # TypeScript 型定義
├── src-tauri/              # Rust バックエンド
│   └── src/
│       ├── commands/       # Tauri commands（フロントエンドAPI）
│       ├── mail_sync/      # IMAP / SMTP / MIME / IDLE
│       ├── classifier/     # LLM抽象レイヤー（Ollama / Claude / Vertex）
│       ├── db/             # SQLite スキーマ・CRUD・FTS5
│       └── models/         # 共有データ型
└── docs/
    ├── adr/                # アーキテクチャ意思決定記録（ADR）
    ├── design/            # 現役の設計書（本体設計・進行中フェーズ）
    ├── plans/             # 現役の実装計画（未完のもの）
    └── archive/           # 実装完了で役目を終えた設計書・計画
```

## 📚 ドキュメント

- [SETUP.md](SETUP.md) — インストールと初期設定
- [FEATURES.md](FEATURES.md) — 機能一覧と使い方
- [docs/adr/](docs/adr/) — 横断的・恒久的なアーキテクチャ意思決定記録（LLM抽象化、クラウド送信境界、機密情報保管、AI-Nativeアーキ、メール同期規約）
- [docs/design/](docs/design/) — 現役の設計書（本体設計は `2026-04-12-pigeon-design.md`）
- [docs/archive/](docs/archive/) — 実装完了済みの設計書・計画（歴史的資料）
- [CONTRIBUTING.md](CONTRIBUTING.md) — 開発への参加方法
- [SECURITY.md](SECURITY.md) — 脆弱性の報告方法とセキュリティ設計方針
- [LICENSE](LICENSE) — MIT License
