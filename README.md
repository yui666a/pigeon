# Pigeon

AIによってメールを案件ごとに自動グルーピングするデスクトップメールクライアント。ローカルLLM（Ollama）を使用してメールを案件単位で自動分類し、「案件 > スレッド > メール」の階層で管理する。

## 必要要件

- [mise](https://mise.jdx.dev/) (推奨) — Rust / Node.js / pnpm を一括管理
- [Rust](https://www.rust-lang.org/tools/install) (1.94+)
- [Node.js](https://nodejs.org/) (22+)
- [pnpm](https://pnpm.io/) (10+)
- [Ollama](https://ollama.ai/) (ローカルLLM、Phase 2 以降で使用)
- Tauri 2 の[システム依存関係](https://v2.tauri.app/start/prerequisites/)

## セットアップ

```bash
# リポジトリをクローン
git clone https://github.com/yui666a/pigeon.git
cd pigeon

# mise でツールチェーンをインストール（Rust, Node.js, pnpm）
mise install

# フロントエンドの依存関係をインストール
pnpm install

# 開発サーバーを起動
pnpm tauri dev
```

## テスト

```bash
# Rust テスト
cd src-tauri && cargo test

# フロントエンド テスト
pnpm test
```

## リント

```bash
cd src-tauri && cargo clippy -- -D warnings
cd src-tauri && cargo fmt -- --check
```

## ビルド

```bash
pnpm tauri build
```

## プロジェクト構成

```
pigeon/
├── src/                    # React フロントエンド
│   ├── components/         # UI コンポーネント（3ペイン構成）
│   ├── stores/             # Zustand ストア
│   └── types/              # TypeScript 型定義
├── src-tauri/              # Rust バックエンド
│   └── src/
│       ├── commands/       # Tauri commands（フロントエンドAPI）
│       ├── db/             # SQLite スキーマ・CRUD
│       ├── mail_sync/      # IMAP クライアント・MIME パーサー
│       └── models/         # 共有データ型
└── docs/                   # 設計書・実装計画
```
