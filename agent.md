# Pigeon - エージェント指示書

## プロジェクト概要

PigeonはAIによってメールを案件ごとに自動グルーピングするデスクトップメールクライアント。
従来のフォルダベースの整理ではなく「案件 > スレッド > メール」の階層でメールを管理する。

詳細な設計は `docs/superpowers/specs/2026-04-12-pigeon-design.md` を参照すること。

## 技術スタック

- **フレームワーク**: Tauri 2
- **バックエンド**: Rust
- **フロントエンド**: React 19 + TypeScript
- **パッケージマネージャ**: pnpm
- **ツールチェーン管理**: mise (`mise.toml` でバージョン固定)
- **DB**: SQLite + FTS5
- **LLM**: Ollama（デフォルト）/ Claude API（オプション）
- **状態管理**: Zustand 5
- **スタイル**: Tailwind CSS v4
- **テスト**: Vitest + React Testing Library (フロント) / cargo test (Rust)
- **D&D**: React DnD (Phase 3 で追加予定)

## 開発ルール

### 設計書ファースト

- コードを変更する前に `docs/superpowers/specs/` 配下の設計書を確認すること
- 設計書に記載のない機能追加や仕様変更を行う場合は、先に設計書を更新すること
- 設計書と実装が矛盾する場合は、設計書を正とし実装を修正すること

### TDD（テスト駆動開発）

- 新機能の実装は必ずテストを先に書く
- Red → Green → Refactor のサイクルを守る
- Rust: `#[cfg(test)]` モジュール内にユニットテスト、`tests/` に統合テスト
- React: Vitest + React Testing Library でコンポーネントテスト

### コミットメッセージ

Conventional Commits 形式を使用する:

```
<type>(<scope>): <description>

<body>
```

type: feat, fix, docs, style, refactor, test, chore
scope: mail-sync, classifier, search, ui, db 等

例:
- `feat(mail-sync): IMAP IDLEによるリアルタイム同期を追加`
- `fix(classifier): 確信度スコアの計算ロジックを修正`
- `test(db): correction_logのCRUD操作テストを追加`

## Rust コーディング規約

### エラーハンドリング

- `unwrap()` / `expect()` はテストコード以外で使用しない
- アプリケーションエラーは `thiserror` で定義する
- Tauri commandsでは `Result<T, String>` を返す

### 命名

- モジュール名: snake_case (`mail_sync`, `classifier`)
- 構造体/列挙型: PascalCase (`MailSummary`, `ClassifyResult`)
- 関数/メソッド: snake_case (`fetch_emails`, `classify_mail`)

### モジュール構成

```
src-tauri/src/
├── main.rs          # エントリポイント
├── commands/        # Tauri commands（UIに公開するAPI）
├── mail_sync/       # IMAP/SMTP/MIMEパース
├── classifier/      # LLM抽象レイヤー
├── search/          # 全文検索
├── db/              # SQLiteスキーマ・CRUD
└── models/          # 共有データ型
```

## React/TypeScript コーディング規約

### コンポーネント設計

- 関数コンポーネントのみ使用（クラスコンポーネント禁止）
- 1ファイル1コンポーネント
- Props は interface で定義する

### 型定義

- `any` は使用しない
- Tauri invoke のレスポンスには必ず型を付ける
- 共通型は `src/types/` にまとめる

### 状態管理

- グローバル状態は Zustand のストアに集約
- コンポーネントローカルの状態は useState
- サーバー状態（メールデータ等）はストア経由で管理

### ディレクトリ構成

```
src/
├── components/      # UIコンポーネント
│   ├── sidebar/     # 左ペイン（案件ツリー）
│   ├── thread-list/ # 中央ペイン（スレッド一覧）
│   ├── mail-view/   # 右ペイン（メール本文）
│   └── common/      # 共通コンポーネント
├── stores/          # Zustand ストア
├── hooks/           # カスタムフック
├── types/           # TypeScript 型定義
├── utils/           # ユーティリティ関数
└── main.tsx         # エントリポイント
```

## セキュリティルール

- パスワード、OAuthトークン、APIキーはOSキーチェーンに保存する。SQLiteに平文で保存しない
- LLMへ送信するデータは件名、送信者、本文冒頭300文字に限定する
- デフォルトLLMはOllama（ローカル）。クラウドAPI選択時はユーザーに警告を表示する
- `.env` ファイルをコミットしない（.gitignoreに記載済み）
