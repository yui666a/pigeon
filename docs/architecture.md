# アーキテクチャ概要

## 構成

モノリシック構成。Tauri 2 単一プロセス内でReact UI + Rustバックエンドが動作する。

```
┌─ Pigeon (Tauri 2 App) ────────────────────────────────────┐
│                                                             │
│  ┌─ React (WebView) ────────────────────────────────────┐   │
│  │  Zustand / Tailwind CSS v4                            │   │
│  │                                                       │   │
│  │  ┌──────────┬───────────────┬───────────────────────┐ │   │
│  │  │ Sidebar  │ ThreadList    │ MailView              │ │   │
│  │  │          │               │                       │ │   │
│  │  │AccountLst│ ThreadItem[]  │ MailHeader / Body     │ │   │
│  │  │AccountFrm│               │                       │ │   │
│  │  └──────────┴───────────────┴───────────────────────┘ │   │
│  └────────────────────┬──────────────────────────────────┘   │
│                       │ invoke()                             │
│  ┌─ Rust Backend ─────▼──────────────────────────────────┐   │
│  │                                                       │   │
│  │  ┌──────────┐ ┌───────────┐ ┌───────────────────────┐│   │
│  │  │MailSync  │ │Classifier │ │SearchEngine           ││   │
│  │  │  ✅ 実装済│ │  未実装    │ │  未実装               ││   │
│  │  └────┬─────┘ └─────┬─────┘ └──────────┬────────────┘│   │
│  │       │             │                  │             │   │
│  │  ┌────▼─────────────▼──────────────────▼───────────┐ │   │
│  │  │                 SQLite DB  ✅ 実装済              │ │   │
│  │  └─────────────────────────────────────────────────┘ │   │
│  └───────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Rustモジュール

| モジュール | 責務 | 状態 |
|-----------|------|------|
| `models` | Account / Mail / Thread 構造体、AppError | Phase 1 実装済 |
| `db` | SQLiteスキーマ管理・accounts/mails CRUD・スレッド構築 | Phase 1 実装済 |
| `mail_sync` | IMAP接続・差分同期・MIMEパース | Phase 1 実装済 |
| `commands` | Tauri commandsとしてReactに公開するAPI群 | Phase 1 実装済 |
| `classifier` | LLM抽象レイヤー（Ollama / Claude API） | Phase 2 |
| `search` | SQLite FTS5を使った全文検索・案件横断検索 | Phase 4 |

## Tauri Commands API

| コマンド | 引数 | 戻り値 | 用途 |
|---------|------|--------|------|
| `create_account` | CreateAccountRequest | Account | アカウント作成 |
| `get_accounts` | — | Account[] | アカウント一覧 |
| `remove_account` | id | — | アカウント削除 |
| `sync_account` | account_id, imap_host, imap_port, username, password | u32 (取得数) | IMAP同期 |
| `get_threads` | account_id, folder | Thread[] | スレッド一覧取得 |

## データフロー

### メール受信（Phase 1 実装済）

```
sync_account コマンド呼び出し
  → IMAP接続 (TLS)
  → UID差分フェッチ (since_uid 以降)
  → MIMEパース (mail-parser)
  → SQLite保存 (INSERT OR REPLACE)
  → get_threads でスレッド構築 (Union-Find + 件名フォールバック)
  → フロントエンドに返却
```

### メール受信 → AI分類（Phase 2 予定）

```
IMAP IDLE (新着検知)
  → MIMEパース
  → SQLite保存 + FTS5インデックス
  → LLM分類 (Ollama)
  → 案件に紐付け (mail_project_assignments)
  → UI更新 (Tauri event → React)
```

### 手動修正 → フィードバック（Phase 3 予定）

```
ユーザーがD&Dで案件を変更
  → mail_project_assignments 更新
  → correction_log に記録
  → 次回LLM呼び出し時にプロンプトに含める
```

## 主要クレート

| 用途 | クレート | 状態 |
|------|---------|------|
| IMAP | `async-imap` + `async-native-tls` | 使用中 |
| MIMEパース | `mail-parser` | 使用中 |
| DB | `rusqlite` (bundled) | 使用中 |
| JSON | `serde` / `serde_json` | 使用中 |
| 非同期 | `tokio` | 使用中 |
| エラー | `thiserror` | 使用中 |
| ID生成 | `uuid` | 使用中 |
| 日時 | `chrono` | 使用中 |
| SMTP | `lettre` | Phase 4 で追加予定 |
| HTTP | `reqwest` | Phase 2 で追加予定 |

## 主要フロントエンドライブラリ

| 用途 | ライブラリ | 状態 |
|------|-----------|------|
| UIフレームワーク | React 19 | 使用中 |
| 状態管理 | Zustand 5 | 使用中 |
| スタイル | Tailwind CSS v4 | 使用中 |
| テスト | Vitest 4 + React Testing Library | 使用中 |
| D&D | React DnD | Phase 3 で追加予定 |
| リッチテキスト | TipTap | Phase 4 で追加予定 |
