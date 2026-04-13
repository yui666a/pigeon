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
│  │  │ Sidebar  │ ThreadList /  │ MailView              │ │   │
│  │  │          │ Unclassified  │                       │ │   │
│  │  │AccountLst│ ThreadItem[]  │ MailHeader / Body     │ │   │
│  │  │ProjectTre│               │                       │ │   │
│  │  │ProjectFrm│               │                       │ │   │
│  │  └──────────┴───────────────┴───────────────────────┘ │   │
│  └────────────────────┬──────────────────────────────────┘   │
│                       │ invoke()                             │
│  ┌─ Rust Backend ─────▼──────────────────────────────────┐   │
│  │                                                       │   │
│  │  ┌──────────┐ ┌───────────┐ ┌───────────────────────┐│   │
│  │  │MailSync  │ │Classifier │ │SearchEngine           ││   │
│  │  │  ✅ 実装済│ │  ✅ 実装済 │ │  未実装               ││   │
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
| `models` | Account / Mail / Thread / Project / ClassifyResult 構造体、AppError | Phase 1 + OAuth + Phase 2 実装済 |
| `db` | SQLiteスキーマ管理・accounts/mails/projects/assignments CRUD・スレッド構築 | V3 マイグレーション済 |
| `mail_sync` | IMAP接続・差分同期・MIMEパース・OAuth 2.0フロー | Phase 1 実装済 + OAuth 実装済 |
| `commands` | Tauri commandsとしてReactに公開するAPI群 | Phase 1 + OAuth + Phase 2 実装済 |
| `secure_store` | Stronghold によるトークン/パスワードの暗号化保存 | OAuth で追加 |
| `classifier` | LLM抽象レイヤー（LlmClassifier trait + OllamaClassifier） | Phase 2 実装済 |
| `search` | SQLite FTS5を使った全文検索・案件横断検索 | Phase 4 |

## Tauri Commands API

### アカウント管理

| コマンド | 引数 | 戻り値 | 用途 |
|---------|------|--------|------|
| `create_account` | CreateAccountRequest | Account | アカウント作成 |
| `get_accounts` | — | Account[] | アカウント一覧 |
| `remove_account` | id | — | アカウント削除（関連メール・案件も削除） |
| `start_oauth` | provider | — | OAuth 認可フロー開始（ブラウザを開く） |
| `handle_oauth_callback` | url | Account | OAuth コールバック処理・アカウント保存 |

### メール同期

| コマンド | 引数 | 戻り値 | 用途 |
|---------|------|--------|------|
| `sync_account` | account_id | u32 (取得数) | IMAP同期（認証情報はバックエンドで解決） |
| `get_threads` | account_id, folder | Thread[] | スレッド一覧取得 |

### 案件管理（Phase 2）

| コマンド | 引数 | 戻り値 | 用途 |
|---------|------|--------|------|
| `create_project` | account_id, name, description?, color? | Project | 案件を手動作成 |
| `get_projects` | account_id | Project[] | アカウントの案件一覧 |
| `update_project` | id, name?, description?, color? | Project | 案件を更新 |
| `archive_project` | id | — | 案件を論理削除 |
| `delete_project` | id | — | 案件を物理削除（CASCADE） |

### AI分類（Phase 2）

| コマンド | 引数 | 戻り値 | 用途 |
|---------|------|--------|------|
| `classify_mail` | mail_id | ClassifyResponse | 1通をLLMで分類 |
| `classify_unassigned` | account_id | — (進捗はTauri events) | 未分類メール全件を分類 |
| `cancel_classification` | — | — | 実行中の分類を中止 |
| `approve_classification` | mail_id, project_id | — | 分類結果を承認または修正 |
| `approve_new_project` | mail_id, project_name, description? | Project | 新規案件提案を承認 |
| `reject_classification` | mail_id | — | 分類結果を破棄 |
| `get_unclassified_mails` | account_id | Mail[] | 未分類メール一覧 |
| `get_mails_by_project` | project_id | Mail[] | 案件に紐づくメール一覧 |

## データフロー

### メール受信（Phase 1 実装済）

```
sync_account コマンド呼び出し (account_id のみ)
  → accounts テーブルからアカウント情報取得
  → provider で分岐:
    Google: Stronghold から OAuth トークン取得 → 必要ならリフレッシュ → XOAUTH2 で IMAP接続
    Other:  Stronghold からパスワード取得 → PLAIN で IMAP接続
  → UID差分フェッチ (since_uid 以降、初回は直近20件)
  → MIMEパース (mail-parser)
  → SQLite保存 (INSERT OR REPLACE)
  → get_threads でスレッド構築 (Union-Find + 件名フォールバック)
  → フロントエンドに返却
```

### AI分類（Phase 2 実装済）

```
classify_unassigned コマンド呼び出し (account_id)
  → 未分類メール取得 (LEFT JOIN mail_project_assignments)
  → Ollama ヘルスチェック (GET /api/tags)
  → メール1通ずつ LLM に分類リクエスト:
    ├─ MailSummary 構築 (件名, 送信者, 日付, 本文冒頭300文字)
    ├─ ProjectSummary 構築 (案件名, 説明, 直近メール件名3件)
    └─ POST /api/chat → JSON応答をパース
  → 確信度で振り分け:
    ├─ >= 0.7: 自動割り当て (assigned_by = 'ai')
    ├─ 0.4〜0.7: 割り当て + ⚠ マーク
    ├─ < 0.4: 未分類のまま
    └─ create: PendingClassifications に保持 → ユーザー承認待ち
  → Tauri events で進捗通知 (classify-progress, classify-complete)
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
| IMAP | `async-imap` 0.11 + `async-native-tls` | 使用中 |
| MIMEパース | `mail-parser` | 使用中 |
| DB | `rusqlite` (bundled) | 使用中 |
| JSON | `serde` / `serde_json` | 使用中 |
| 非同期 | `tokio` | 使用中 |
| エラー | `thiserror` | 使用中 |
| ID生成 | `uuid` | 使用中 |
| 日時 | `chrono` | 使用中 |
| HTTP | `reqwest` | 使用中（OAuth トークン交換 + Ollama API） |
| セキュアストレージ | `iota-stronghold` | 使用中（トークン/パスワード暗号化保存） |
| Deep Link | `tauri-plugin-deep-link` | 使用中（OAuth コールバック受信） |
| PKCE/暗号 | `sha2` + `rand` + `base64` | 使用中（OAuth PKCE フロー） |
| 非同期trait | `async-trait` | 使用中（LlmClassifier trait） |
| SMTP | `lettre` | Phase 4 で追加予定 |

## 主要フロントエンドライブラリ

| 用途 | ライブラリ | 状態 |
|------|-----------|------|
| UIフレームワーク | React 19 | 使用中 |
| 状態管理 | Zustand 5 | 使用中 |
| スタイル | Tailwind CSS v4 | 使用中 |
| テスト | Vitest 4 + React Testing Library | 使用中 |
| D&D | React DnD | Phase 3 で追加予定 |
| リッチテキスト | TipTap | Phase 4 で追加予定 |

## フロントエンドストア

| ストア | 説明 |
|-------|------|
| `accountStore` | アカウント一覧、OAuth フロー管理 |
| `mailStore` | スレッド一覧、メール同期 |
| `projectStore` | 案件一覧、CRUD操作 |
| `classifyStore` | 分類状態、進捗、結果、承認/却下操作 |

## DBスキーマバージョン

| Version | 内容 |
|---------|------|
| V1 | accounts, mails, settings テーブル |
| V2 | accounts に provider カラム追加 |
| V3 | projects, mail_project_assignments, correction_log テーブル + アカウント整合性トリガー |
