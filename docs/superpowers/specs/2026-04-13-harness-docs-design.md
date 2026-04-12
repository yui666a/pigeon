# Pigeon ハーネスドキュメント整備 設計書

## 概要

Pigeonプロジェクトの開発を円滑に進めるために、CLAUDE.md / agent.md を中心としたハーネスドキュメント一式を整備する。

## 作成ファイル

### 1. CLAUDE.md

`@agent.md` への参照のみ。AIエージェントが読み込む際のエントリポイント。

### 2. agent.md

プロジェクトの全ルールを集約するファイル。以下のセクションで構成:

1. **プロジェクト概要** - Pigeonの目的と概要
2. **技術スタック** - Tauri 2 / Rust / React / TypeScript / SQLite / Ollama
3. **開発ルール**
   - 設計書ファースト: コード変更前に `docs/superpowers/specs/` の設計書を確認・更新
   - TDD: テストを先に書いてからプロダクションコードを実装
   - コミットメッセージ: Conventional Commits 形式
4. **Rust コーディング規約** - エラーハンドリング、命名、モジュール構成
5. **React/TypeScript コーディング規約** - コンポーネント設計、型定義、状態管理
6. **ディレクトリ構成** - src/ と src-tauri/ の役割
7. **セキュリティルール** - 認証情報の扱い、LLMへのデータ送信制限

### 3. README.md

- プロジェクト概要（1段落）
- 必要要件（Rust、Node.js、Ollama）
- セットアップ手順
- 開発サーバー起動方法

### 4. .gitignore

- Rust: target/、Cargo.lock（ライブラリの場合。アプリなので含める）
- Node.js: node_modules/、dist/
- Tauri: src-tauri/target/
- OS: .DS_Store、Thumbs.db
- IDE: .vscode/、.idea/
- 環境: .env

### 5. CONTRIBUTING.md

- 開発フロー（設計書確認 → テスト → 実装 → レビュー）
- ブランチ戦略（main + feature branches）
- コミット規約（Conventional Commits）
- PR作成ルール

### 6. docs/architecture.md

- モジュール構成図（設計書セクション1から抽出）
- データフロー概要
- 主要クレート/ライブラリ一覧
