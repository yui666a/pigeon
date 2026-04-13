# Pigeon - AI案件分類メーラー 設計書

## 概要

Pigeonは、AIによってメールを案件ごとに自動グルーピングするデスクトップメールクライアント。従来のフォルダベースの整理ではなく「案件 > スレッド > メール」の階層でメールを管理し、案件単位での情報アクセスを実現する。

### 基本情報

| 項目 | 内容 |
|------|------|
| アプリ名 | Pigeon |
| フレームワーク | Tauri 2 + React (TypeScript) |
| バックエンド | Rust |
| DB | SQLite + FTS5 |
| LLM | Ollama (デフォルト) / Claude API (オプション) |
| 対象ユーザー | 自分一人 |
| 対象メールサーバー | 自社メールサーバー (IMAP/SMTP) |
| 認証方式 | PLAIN/LOGIN + OAuth 2.0 |
| アカウント | 複数アカウント対応 |

---

## 1. アーキテクチャ

モノリシック構成。Tauri単一プロセス内でReact UI + Rustバックエンドが動作する。

```
┌─ Pigeon (Tauri App) ───────────────────────────────────────┐
│                                                             │
│  ┌─ React (WebView) ────────────────────────────────────┐   │
│  │  状態管理: Zustand                                    │   │
│  │  スタイル: Tailwind CSS                               │   │
│  │  D&D: React DnD                                      │   │
│  │                                                       │   │
│  │  ┌──────────┬───────────────┬───────────────────────┐ │   │
│  │  │ Sidebar  │ ThreadList    │ MailView              │ │   │
│  │  │          │               │                       │ │   │
│  │  │AccountSw │ ThreadItem[]  │ Header / Body /       │ │   │
│  │  │ProjectTr │               │ Attachments / Actions │ │   │
│  │  │SearchBar │               │                       │ │   │
│  │  └──────────┴───────────────┴───────────────────────┘ │   │
│  └────────────────────┬──────────────────────────────────┘   │
│                       │ invoke()                             │
│  ┌─ Rust Backend ─────▼──────────────────────────────────┐   │
│  │                                                       │   │
│  │  ┌──────────┐ ┌───────────┐ ┌───────────────────────┐│   │
│  │  │MailSync  │ │Classifier │ │SearchEngine           ││   │
│  │  │          │ │           │ │                       ││   │
│  │  │IMAP接続   │ │LLM trait  │ │SQLite FTS5            ││   │
│  │  │SMTP送信   │ │ ├Ollama   │ │全文検索                ││   │
│  │  │MIME解析   │ │ └Claude   │ │案件横断検索             ││   │
│  │  └────┬─────┘ └─────┬─────┘ └──────────┬────────────┘│   │
│  │       │             │                  │             │   │
│  │  ┌────▼─────────────▼──────────────────▼───────────┐ │   │
│  │  │                 SQLite DB                        │ │   │
│  │  └─────────────────────────────────────────────────┘ │   │
│  └───────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Rustモジュール構成

| モジュール | 責務 |
|-----------|------|
| `mail_sync` | IMAP接続・差分同期・SMTP送信・MIMEパース |
| `classifier` | LLM抽象レイヤー。Ollama / Claude API を同一traitで切り替え |
| `search` | SQLite FTS5を使った全文検索・案件横断検索 |
| `db` | SQLiteスキーマ管理・CRUD操作 |
| `commands` | Tauri commandsとしてReactに公開するAPI群 |

### フロントエンド技術スタック

| ライブラリ | 用途 |
|-----------|------|
| Zustand | 軽量な状態管理 |
| Tailwind CSS | ユーティリティベースのスタイリング |
| React DnD | ドラッグ&ドロップ（案件間のメール移動） |

---

## 2. データモデル

```sql
-- アカウント管理
CREATE TABLE accounts (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    email       TEXT NOT NULL,
    imap_host   TEXT NOT NULL,
    imap_port   INTEGER NOT NULL DEFAULT 993,
    smtp_host   TEXT NOT NULL,
    smtp_port   INTEGER NOT NULL DEFAULT 587,
    auth_type   TEXT NOT NULL CHECK(auth_type IN ('plain', 'oauth2')),
    credentials BLOB NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- メール
CREATE TABLE mails (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(id),
    folder       TEXT NOT NULL,
    message_id   TEXT NOT NULL,
    in_reply_to  TEXT,
    references   TEXT,
    from_addr    TEXT NOT NULL,
    to_addr      TEXT NOT NULL,
    cc_addr      TEXT,
    subject      TEXT NOT NULL,
    body_text    TEXT,
    body_html    TEXT,
    date         DATETIME NOT NULL,
    has_attachments BOOLEAN DEFAULT FALSE,
    raw_size     INTEGER,
    uid          INTEGER NOT NULL,
    flags        TEXT,
    fetched_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 添付ファイル
CREATE TABLE attachments (
    id          TEXT PRIMARY KEY,
    mail_id     TEXT NOT NULL REFERENCES mails(id),
    filename    TEXT NOT NULL,
    mime_type   TEXT NOT NULL,
    size        INTEGER,
    file_path   TEXT
);

-- 案件
CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    color       TEXT,
    is_archived BOOLEAN DEFAULT FALSE,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- メール→案件の紐付け
CREATE TABLE mail_project_assignments (
    mail_id        TEXT PRIMARY KEY REFERENCES mails(id),
    project_id     TEXT NOT NULL REFERENCES projects(id),
    assigned_by    TEXT NOT NULL CHECK(assigned_by IN ('ai', 'user')),
    confidence     REAL,
    corrected_from TEXT,
    created_at     DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 手動修正履歴 (AIフィードバック用)
CREATE TABLE correction_log (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    mail_id        TEXT NOT NULL,
    from_project   TEXT,
    to_project     TEXT NOT NULL,
    corrected_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 全文検索用 (FTS5)
CREATE VIRTUAL TABLE fts_mails USING fts5(
    mail_id UNINDEXED,
    subject,
    body_text,
    from_addr,
    to_addr
);

-- 設定
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### 設計ポイント

- `mails.in_reply_to` と `mails.references` でスレッドを構築（RFC 2822準拠）
- `credentials` はOSキーチェーンで保護（SQLiteに平文保存しない）
- `fts_mails` はメール保存時にトリガーで自動同期
- `settings` テーブルでLLMプロバイダ、初回同期期間等をユーザーが設定可能

---

## 3. AI分類フロー

### 分類パイプライン

```
メール到着 (IMAP IDLE or ポーリング)
    │
    ▼
MIMEパース → SQLiteに保存 → FTS5にインデックス
    │
    ▼
分類リクエスト構築
    ├─ メール情報: 件名, 送信者, 本文冒頭300文字
    ├─ 既存案件リスト: 名前 + 説明 + 最近のメール件名3件
    └─ 修正履歴: 直近20件の correction_log
    │
    ▼
LLM呼び出し (Ollama or Claude API)
    │
    ▼
レスポンス (JSON)
    ├─ { "action": "assign", "project_id": "xxx", "confidence": 0.85 }
    ├─ { "action": "create", "project_name": "○○案件", "description": "...", "confidence": 0.78 }
    └─ { "action": "unclassified", "confidence": 0.30 }
    │
    ▼
confidence による振り分け
    ├─ >= 0.7  → 自動分類、UIに反映
    ├─ 0.4〜0.7 → 分類するが ⚠マーク付き
    └─ < 0.4  → 「未分類」に格納、通知
```

### LLM抽象レイヤー

```rust
trait LlmClassifier {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, ClassifyError>;
}

struct OllamaClassifier { endpoint: String, model: String }
struct ClaudeClassifier { api_key: String, model: String }
```

### LLMプロバイダ優先順位

| 優先度 | プロバイダ | 備考 |
|-------|-----------|------|
| デフォルト | Ollama (ローカルLLM) | データが外部に出ない。セキュリティ最優先 |
| オプション | Claude API等 | ユーザーが明示的に選択した場合のみ。警告表示あり |

### バッチ分類（初回同期時）

- 10通ずつバッチでLLMに送信
- 時系列順（古い方から）に処理
- 進捗バーをUIに表示
- 途中でアプリを閉じても再開可能（分類済みフラグで管理）

### フィードバックループ

ユーザーが手動で案件を修正した履歴（correction_log）を、次回以降のLLMプロンプトにfew-shot exampleとして含める。修正を重ねるほど分類精度が向上する。

---

## 4. メール同期

### IMAP同期戦略

> **TODO（現状の暫定実装）**: 現在は初回同期で直近100件のみ取得している。以下の段階的同期に改修すること：
> 1. 初回同期: 過去3ヶ月分のメールを取得（IMAP SINCE で日付フィルタ）
> 2. ユーザーが「過去メールをすべて取得」を承認した場合のみ、全件取得を実行（バッチ処理 + 進捗バー）
> 3. 全件取得はバックグラウンドで段階的に行い、アプリの操作をブロックしない

```
初回同期:
  1. IMAP接続 → フォルダ一覧取得 (LIST)
  2. INBOX, Sent を優先同期
  3. 過去3ヶ月分のメールを取得（デフォルト）
  4. UID順に取得 → SQLiteに保存 → FTS5にインデックス
  5. 完了後、AI分類パイプラインにキュー投入
  6. ユーザー承認後、残りの過去メールをバックグラウンドで段階的に取得

通常同期 (アプリ起動中):
  IMAP IDLE で待機 (プッシュ通知相当)
  ├─ 新着メール → 即取得 → 保存 → AI分類
  ├─ フラグ変更 (既読等) → ローカルDB更新
  └─ 削除 → ローカルDBに反映

アプリ起動時:
  前回同期以降の差分を UID ベースで取得
  (IMAP UIDVALIDITY + 最後に取得した UID で差分判定)
```

### 双方向同期

| 操作 | 方向 | 処理 |
|------|------|------|
| 新着メール | サーバー→ローカル | IDLE / ポーリングで検知、取得して保存 |
| 既読にする | ローカル→サーバー | IMAP STORE で \Seen フラグ設定 |
| メール送信 | ローカル→サーバー | SMTP送信 → Sentフォルダにコピー |
| メール削除 | ローカル→サーバー | IMAP STORE で \Deleted → EXPUNGE |

### アカウント別の接続管理

各アカウントが独立したIMAP接続を保持。IMAP IDLEで新着を待機。接続断時はexponential backoffで自動リトライ。

### スレッド構築

RFC 2822の `In-Reply-To` と `References` ヘッダーでスレッドツリーを構築。ヘッダーが欠落している場合は件名ベースのフォールバックマッチング（`Re:` `Fwd:` を除去して比較）を行う。

---

## 5. UI設計

### 画面構成

```
┌─────────────────────────────────────────────────────────────┐
│  Pigeon                                    ─  □  ×         │
├────────────┬───────────────────┬─────────────────────────────┤
│            │                   │ From: tanaka@example.com    │
│ 🔍 検索    │  ○ 見積もりの件   │ To: me@company.com          │
│            │    田中太郎 04/10 │ Date: 2026-04-10 14:30      │
│ ▼ Account A│                  │ Cc: suzuki@example.com      │
│ ▼ Account B│  ○ 納期変更の..  │─────────────────────────────│
│            │    佐藤花子 04/09 │                             │
│ ─────────  │                  │ お世話になっております。     │
│ ▶ 案件A (8)│ ⚠ サーバー移行.. │                             │
│ ▶ 案件B (3)│    山田次郎 04/08 │ 見積もりの件について        │
│ ▶ 案件C (5)│                  │ ご連絡いたします。          │
│   案件D (2)│                  │                             │
│ ─────────  │                  │ ...本文...                  │
│ ⚠未分類 (2)│                  │                             │
│            │                  │ ─────────────────────────── │
│            │                  │ 📎 見積書.pdf (245KB)  [DL] │
│            │                  │ 📎 仕様書.xlsx (180KB) [DL] │
│            │                  │                             │
│ ─────────  │                  │ [返信] [全員に返信] [転送]   │
│ ⚙ 設定     │                  │                             │
├────────────┴───────────────────┴─────────────────────────────┤
│ 同期中... Account A (3/120)                                  │
└─────────────────────────────────────────────────────────────┘
```

### 主要画面

| 画面 | 内容 |
|------|------|
| メイン (3ペイン) | 左: 案件ツリー、中: スレッド一覧、右: メール本文 |
| 設定 | アカウント管理、LLMプロバイダ選択、初回同期期間、テーマ |
| メール作成 | 新規作成・返信・転送。リッチテキストエディタ |
| 検索結果 | 全文検索結果。案件名・ヒット箇所をハイライト表示 |

### インタラクション

| 操作 | アクション |
|------|-----------|
| 案件クリック | 中央ペインにその案件のスレッド一覧を表示 |
| スレッドクリック | 右ペインにスレッド内のメールを時系列表示 |
| メールをドラッグ | 別の案件にドロップで手動分類。correction_logに記録 |
| 案件を右クリック | 「名前変更」「色変更」「アーカイブ」「マージ」 |
| 未分類メールを右クリック | 「既存案件に分類」「新しい案件として作成」 |
| ⚠マーク | 確信度が低い分類。クリックで「正しい / 修正する」を選択 |

### キーボードショートカット

| キー | 操作 |
|------|------|
| `j` / `k` | 次/前のメール |
| `n` | 新規メール作成 |
| `r` | 返信 |
| `a` | 全員に返信 |
| `f` | 転送 |
| `/` | 検索にフォーカス |
| `e` | アーカイブ |

---

## 6. セキュリティ

### 認証情報の保存

OSネイティブのキーチェーンを使用:

- macOS: Keychain Services
- Windows: Credential Manager
- Linux: libsecret (GNOME Keyring)

Tauriの `tauri-plugin-stronghold` またはOSネイティブAPIで保護。SQLiteにはパスワード・トークン・APIキーを平文で保存しない。

### LLMへの送信データ

| 項目 | 送信する | 送信しない |
|------|---------|-----------|
| 件名 | o | |
| 送信者 | o | |
| 本文冒頭300文字 | o | |
| 本文全文 | | x |
| 添付ファイル | | x |
| メールアドレス一覧 | | x |

デフォルトはOllama（ローカルLLM）のため、ネットワーク外にデータが一切出ない。クラウドAPI選択時は警告を明示表示する。

### ローカルデータ

- SQLiteファイル: `~/Library/Application Support/Pigeon/` (macOS)
- アプリ削除時にデータも削除するかユーザーに確認

---

## 7. フェーズ分割

### Phase 1: 基盤（メールの受信・表示）

- Tauriプロジェクトセットアップ
- アカウント設定画面（IMAP/SMTP接続情報の入力・保存）
- IMAP接続、メール取得、SQLiteに保存
- 3ペインUIでメール一覧・本文表示
- スレッド構築（In-Reply-To / References）

### Phase 2: AI分類

- Ollama連携（LLM trait + OllamaClassifier実装）
- 新着メールの自動分類
- 案件ツリー表示
- 確信度表示（⚠マーク）

### Phase 3: 手動修正・フィードバック

- ドラッグ&ドロップによる案件移動
- 右クリックメニュー（新規案件作成、マージ）
- correction_log記録
- 修正履歴をプロンプトに反映

### Phase 4: 検索・送信

- SQLite FTS5で全文検索
- 案件横断検索
- SMTP送信（新規・返信・転送）
- メール作成エディタ

### Phase 5: 過去メール分類・仕上げ

- 初回同期時のバッチ分類（期間はユーザーが設定画面で指定）
- 進捗バー表示
- Claude API対応（オプション）
- 添付ファイルの一覧表示・ダウンロード
- キーボードショートカット
- ~~OAuth 2.0認証対応~~ → **実装済（Phase 5 から前倒し）**。詳細は `2026-04-13-oauth-support-design.md` を参照

### Phase 6: 将来（スコープ外）

- 添付ファイルの中身をAI分類材料に
- デスクトップ通知
- テーマ切り替え（ダーク/ライト）

---

## 8. 主要クレート・ライブラリ

### Rust

| 用途 | クレート |
|------|---------|
| IMAP | `async-imap` |
| SMTP | `lettre` |
| MIMEパース | `mail-parser` |
| DB | `rusqlite` |
| HTTP (LLM呼び出し) | `reqwest` |
| JSON | `serde` / `serde_json` |
| 非同期ランタイム | `tokio` |

### React (TypeScript)

| 用途 | ライブラリ |
|------|-----------|
| 状態管理 | Zustand |
| スタイル | Tailwind CSS |
| D&D | React DnD |
| リッチテキスト | TipTap (メール作成用) |
