# アーキテクチャ概要

## 構成

モノリシック構成。Tauri単一プロセス内でReact UI + Rustバックエンドが動作する。

```
┌─ Pigeon (Tauri App) ───────────────────────────────────────┐
│                                                             │
│  ┌─ React (WebView) ────────────────────────────────────┐   │
│  │  Zustand / Tailwind CSS / React DnD                   │   │
│  │                                                       │   │
│  │  ┌──────────┬───────────────┬───────────────────────┐ │   │
│  │  │ Sidebar  │ ThreadList    │ MailView              │ │   │
│  │  └──────────┴───────────────┴───────────────────────┘ │   │
│  └────────────────────┬──────────────────────────────────┘   │
│                       │ invoke()                             │
│  ┌─ Rust Backend ─────▼──────────────────────────────────┐   │
│  │                                                       │   │
│  │  ┌──────────┐ ┌───────────┐ ┌───────────────────────┐│   │
│  │  │MailSync  │ │Classifier │ │SearchEngine           ││   │
│  │  └────┬─────┘ └─────┬─────┘ └──────────┬────────────┘│   │
│  │       │             │                  │             │   │
│  │  ┌────▼─────────────▼──────────────────▼───────────┐ │   │
│  │  │                 SQLite DB                        │ │   │
│  │  └─────────────────────────────────────────────────┘ │   │
│  └───────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Rustモジュール

| モジュール | 責務 |
|-----------|------|
| `mail_sync` | IMAP接続・差分同期・SMTP送信・MIMEパース |
| `classifier` | LLM抽象レイヤー（Ollama / Claude API） |
| `search` | SQLite FTS5を使った全文検索・案件横断検索 |
| `db` | SQLiteスキーマ管理・CRUD操作 |
| `commands` | Tauri commandsとしてReactに公開するAPI群 |

## データフロー

### メール受信 → AI分類

```
IMAP IDLE (新着検知)
  → MIMEパース
  → SQLite保存 + FTS5インデックス
  → LLM分類 (Ollama)
  → 案件に紐付け (mail_project_assignments)
  → UI更新 (Tauri event → React)
```

### 手動修正 → フィードバック

```
ユーザーがD&Dで案件を変更
  → mail_project_assignments 更新
  → correction_log に記録
  → 次回LLM呼び出し時にプロンプトに含める
```

## 主要クレート

| 用途 | クレート |
|------|---------|
| IMAP | `async-imap` |
| SMTP | `lettre` |
| MIMEパース | `mail-parser` |
| DB | `rusqlite` |
| HTTP | `reqwest` |
| JSON | `serde` / `serde_json` |
| 非同期 | `tokio` |
| エラー | `thiserror` |

## 主要フロントエンドライブラリ

| 用途 | ライブラリ |
|------|-----------|
| 状態管理 | Zustand |
| スタイル | Tailwind CSS |
| D&D | React DnD |
| テスト | Vitest + React Testing Library |
| リッチテキスト | TipTap |
