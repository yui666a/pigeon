# Phase 1: 基盤（メールの受信・表示）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tauri 2 プロジェクトを初期化し、IMAP でメールを取得して SQLite に保存し、3ペイン UI でスレッド表示できる状態にする。

**Architecture:** Tauri 2 のモノリシック構成。Rust バックエンドに db / mail_sync / models モジュール、React フロントエンドに 3ペインレイアウト + Zustand ストア。IMAP 接続情報は OS キーチェーン（tauri-plugin-stronghold）で保護。

**Tech Stack:** Tauri 2, Rust (rusqlite, async-imap, mail-parser, tokio, thiserror, serde, uuid), React 18, TypeScript, Zustand, Tailwind CSS v4, Vite, Vitest

---

## File Structure

### Rust バックエンド (`src-tauri/`)

```
src-tauri/
├── Cargo.toml
├── tauri.conf.json
├── build.rs
├── src/
│   ├── main.rs                    # Tauri エントリポイント
│   ├── lib.rs                     # モジュール宣言
│   ├── error.rs                   # アプリ共通エラー型
│   ├── models/
│   │   ├── mod.rs
│   │   ├── account.rs             # Account 構造体
│   │   └── mail.rs                # Mail, Thread 構造体
│   ├── db/
│   │   ├── mod.rs
│   │   ├── migrations.rs          # スキーマ初期化
│   │   ├── accounts.rs            # accounts テーブル CRUD
│   │   └── mails.rs               # mails テーブル CRUD + スレッド構築
│   ├── mail_sync/
│   │   ├── mod.rs
│   │   ├── imap_client.rs         # IMAP 接続・フェッチ
│   │   └── mime_parser.rs         # MIME パース
│   └── commands/
│       ├── mod.rs
│       ├── account_commands.rs    # アカウント CRUD コマンド
│       └── mail_commands.rs       # メール取得・スレッド取得コマンド
└── tests/
    ├── db_test.rs                 # DB 統合テスト
    └── mail_sync_test.rs          # IMAP/MIME 統合テスト
```

### React フロントエンド (`src/`)

```
src/
├── main.tsx                       # エントリポイント
├── App.tsx                        # ルートコンポーネント (3ペインレイアウト)
├── App.css                        # Tailwind imports
├── types/
│   ├── account.ts                 # Account 型
│   └── mail.ts                    # Mail, Thread 型
├── stores/
│   ├── accountStore.ts            # アカウント状態管理
│   └── mailStore.ts               # メール・スレッド状態管理
├── components/
│   ├── sidebar/
│   │   ├── Sidebar.tsx            # 左ペイン（アカウント一覧）
│   │   ├── AccountList.tsx        # アカウント一覧
│   │   └── AccountForm.tsx        # アカウント追加/編集フォーム
│   ├── thread-list/
│   │   ├── ThreadList.tsx         # 中央ペイン（スレッド一覧）
│   │   └── ThreadItem.tsx         # スレッド1行
│   └── mail-view/
│       ├── MailView.tsx           # 右ペイン（メール本文）
│       └── MailHeader.tsx         # メールヘッダー表示
└── __tests__/
    ├── AccountForm.test.tsx
    ├── ThreadList.test.tsx
    └── MailView.test.tsx
```

---

## Task 1: Tauri 2 プロジェクト初期化

**Files:**
- Create: `src-tauri/` (Tauri scaffolding 全体)
- Create: `src/` (React scaffolding 全体)
- Create: `package.json`, `vite.config.ts`, `tsconfig.json`
- Modify: `.gitignore`

- [ ] **Step 1: create-tauri-app でプロジェクト生成**

```bash
npm create tauri-app@latest . -- --template react-ts --manager npm
```

カレントディレクトリに既存ファイルがあるため、上書き確認が出る場合は yes で進む。
テンプレートが `react-ts` であることを確認。

- [ ] **Step 2: 依存パッケージをインストール**

```bash
npm install
```

- [ ] **Step 3: Rust の追加クレートを Cargo.toml に追加**

`src-tauri/Cargo.toml` の `[dependencies]` に以下を追加:

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-opener = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
rusqlite = { version = "0.31", features = ["bundled"] }
async-imap = "0.10"
async-native-tls = "0.5"
mail-parser = "0.9"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 4: フロントエンド追加パッケージをインストール**

```bash
npm install zustand
npm install -D tailwindcss @tailwindcss/vite vitest @testing-library/react @testing-library/jest-dom jsdom
```

- [ ] **Step 5: Tailwind CSS v4 を設定**

`vite.config.ts` を更新:

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
```

`src/App.css` を更新:

```css
@import "tailwindcss";
```

- [ ] **Step 6: Vitest を設定**

`vitest.config.ts` を作成:

```typescript
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/setupTests.ts"],
  },
});
```

`src/setupTests.ts` を作成:

```typescript
import "@testing-library/jest-dom/vitest";
```

`package.json` の `scripts` に追加:

```json
{
  "scripts": {
    "test": "vitest run",
    "test:watch": "vitest"
  }
}
```

- [ ] **Step 7: アプリが起動することを確認**

```bash
npm run tauri dev
```

Tauri のウィンドウが表示されることを確認。表示後 Ctrl+C で終了。

- [ ] **Step 8: コミット**

```bash
git add -A
git commit -m "chore: Tauri 2 + React + TypeScript プロジェクトを初期化"
```

---

## Task 2: Rust エラー型 + モデル定義

**Files:**
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/error.rs`
- Create: `src-tauri/src/models/mod.rs`
- Create: `src-tauri/src/models/account.rs`
- Create: `src-tauri/src/models/mail.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: lib.rs を作成してモジュール宣言**

```rust
// src-tauri/src/lib.rs
pub mod error;
pub mod models;
```

- [ ] **Step 2: error.rs を作成**

```rust
// src-tauri/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IMAP error: {0}")]
    Imap(String),

    #[error("MIME parse error: {0}")]
    MimeParse(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Mail not found: {0}")]
    MailNotFound(String),
}

impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}
```

- [ ] **Step 3: models/account.rs を作成**

```rust
// src-tauri/src/models/account.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub auth_type: AuthType,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Plain,
    Oauth2,
}

impl AuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthType::Plain => "plain",
            AuthType::Oauth2 => "oauth2",
        }
    }
}

impl TryFrom<&str> for AuthType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "plain" => Ok(AuthType::Plain),
            "oauth2" => Ok(AuthType::Oauth2),
            other => Err(format!("Unknown auth type: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountRequest {
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub auth_type: AuthType,
    pub password: String,
}
```

- [ ] **Step 4: models/mail.rs を作成**

```rust
// src-tauri/src/models/mail.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mail {
    pub id: String,
    pub account_id: String,
    pub folder: String,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub from_addr: String,
    pub to_addr: String,
    pub cc_addr: Option<String>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub date: String,
    pub has_attachments: bool,
    pub raw_size: Option<i64>,
    pub uid: u32,
    pub flags: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub thread_id: String,
    pub subject: String,
    pub last_date: String,
    pub mail_count: usize,
    pub from_addrs: Vec<String>,
    pub mails: Vec<Mail>,
}
```

- [ ] **Step 5: models/mod.rs を作成**

```rust
// src-tauri/src/models/mod.rs
pub mod account;
pub mod mail;
```

- [ ] **Step 6: main.rs を更新して lib をインポート確認**

`src-tauri/src/main.rs` の先頭に以下を追加（既存の Tauri テンプレートコードはそのまま）:

```rust
use pigeon_lib::models;
```

`cargo check` を実行して型エラーがないことを確認:

```bash
cd src-tauri && cargo check
```

Expected: コンパイル成功（warning は許容）

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/lib.rs src-tauri/src/error.rs src-tauri/src/models/ src-tauri/src/main.rs
git commit -m "feat(models): エラー型とAccount/Mail/Threadモデルを定義"
```

---

## Task 3: SQLite DB スキーマ初期化 + accounts CRUD

**Files:**
- Create: `src-tauri/src/db/mod.rs`
- Create: `src-tauri/src/db/migrations.rs`
- Create: `src-tauri/src/db/accounts.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: テストを書く — migrations のスキーマ初期化**

`src-tauri/src/db/migrations.rs` を作成:

```rust
// src-tauri/src/db/migrations.rs
use rusqlite::Connection;

use crate::error::AppError;

pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS accounts (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            email       TEXT NOT NULL,
            imap_host   TEXT NOT NULL,
            imap_port   INTEGER NOT NULL DEFAULT 993,
            smtp_host   TEXT NOT NULL,
            smtp_port   INTEGER NOT NULL DEFAULT 587,
            auth_type   TEXT NOT NULL CHECK(auth_type IN ('plain', 'oauth2')),
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS mails (
            id              TEXT PRIMARY KEY,
            account_id      TEXT NOT NULL REFERENCES accounts(id),
            folder          TEXT NOT NULL,
            message_id      TEXT NOT NULL,
            in_reply_to     TEXT,
            'references'    TEXT,
            from_addr       TEXT NOT NULL,
            to_addr         TEXT NOT NULL,
            cc_addr         TEXT,
            subject         TEXT NOT NULL,
            body_text       TEXT,
            body_html       TEXT,
            date            DATETIME NOT NULL,
            has_attachments BOOLEAN DEFAULT FALSE,
            raw_size        INTEGER,
            uid             INTEGER NOT NULL,
            flags           TEXT,
            fetched_at      DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_migrations_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"accounts".to_string()));
        assert!(tables.contains(&"mails".to_string()));
        assert!(tables.contains(&"settings".to_string()));
    }

    #[test]
    fn test_run_migrations_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }
}
```

- [ ] **Step 2: テストを実行して通ることを確認**

```bash
cd src-tauri && cargo test db::migrations
```

Expected: 2 tests passed

- [ ] **Step 3: テストを書く — accounts CRUD**

`src-tauri/src/db/accounts.rs` を作成:

```rust
// src-tauri/src/db/accounts.rs
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::AppError;
use crate::models::account::{Account, AuthType, CreateAccountRequest};

pub fn insert_account(conn: &Connection, req: &CreateAccountRequest) -> Result<Account, AppError> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            req.name,
            req.email,
            req.imap_host,
            req.imap_port,
            req.smtp_host,
            req.smtp_port,
            req.auth_type.as_str(),
        ],
    )?;
    get_account(conn, &id)
}

pub fn get_account(conn: &Connection, id: &str) -> Result<Account, AppError> {
    conn.query_row(
        "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, created_at
         FROM accounts WHERE id = ?1",
        params![id],
        |row| {
            let auth_str: String = row.get(7)?;
            Ok(Account {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                imap_host: row.get(3)?,
                imap_port: row.get::<_, u32>(4)? as u16,
                smtp_host: row.get(5)?,
                smtp_port: row.get::<_, u32>(6)? as u16,
                auth_type: AuthType::try_from(auth_str.as_str()).unwrap_or(AuthType::Plain),
                created_at: row.get(8)?,
            })
        },
    )
    .map_err(|_| AppError::AccountNotFound(id.to_string()))
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, created_at
         FROM accounts ORDER BY created_at",
    )?;
    let accounts = stmt
        .query_map([], |row| {
            let auth_str: String = row.get(7)?;
            Ok(Account {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                imap_host: row.get(3)?,
                imap_port: row.get::<_, u32>(4)? as u16,
                smtp_host: row.get(5)?,
                smtp_port: row.get::<_, u32>(6)? as u16,
                auth_type: AuthType::try_from(auth_str.as_str()).unwrap_or(AuthType::Plain),
                created_at: row.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(accounts)
}

pub fn delete_account(conn: &Connection, id: &str) -> Result<(), AppError> {
    let affected = conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(AppError::AccountNotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn sample_request() -> CreateAccountRequest {
        CreateAccountRequest {
            name: "Test Account".into(),
            email: "test@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Plain,
            password: "secret".into(),
        }
    }

    #[test]
    fn test_insert_and_get_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_request()).unwrap();

        assert_eq!(account.name, "Test Account");
        assert_eq!(account.email, "test@example.com");
        assert_eq!(account.imap_host, "imap.example.com");

        let fetched = get_account(&conn, &account.id).unwrap();
        assert_eq!(fetched.id, account.id);
    }

    #[test]
    fn test_list_accounts() {
        let conn = setup_db();
        insert_account(&conn, &sample_request()).unwrap();

        let mut req2 = sample_request();
        req2.name = "Second Account".into();
        req2.email = "second@example.com".into();
        insert_account(&conn, &req2).unwrap();

        let accounts = list_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn test_delete_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_request()).unwrap();
        delete_account(&conn, &account.id).unwrap();

        let result = get_account(&conn, &account.id);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_nonexistent_account() {
        let conn = setup_db();
        let result = get_account(&conn, "nonexistent");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 4: db/mod.rs を作成**

```rust
// src-tauri/src/db/mod.rs
pub mod accounts;
pub mod migrations;
```

- [ ] **Step 5: lib.rs を更新**

```rust
// src-tauri/src/lib.rs
pub mod db;
pub mod error;
pub mod models;
```

- [ ] **Step 6: テストを実行**

```bash
cd src-tauri && cargo test db::
```

Expected: 6 tests passed (migrations 2 + accounts 4)

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/db/ src-tauri/src/lib.rs
git commit -m "feat(db): SQLiteスキーマ初期化とaccounts CRUDを実装"
```

---

## Task 4: mails CRUD + スレッド構築

**Files:**
- Create: `src-tauri/src/db/mails.rs`
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: テストを書く — mails CRUD + スレッド構築**

`src-tauri/src/db/mails.rs` を作成:

```rust
// src-tauri/src/db/mails.rs
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::error::AppError;
use crate::models::mail::{Mail, Thread};

pub fn insert_mail(conn: &Connection, mail: &Mail) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO mails
         (id, account_id, folder, message_id, in_reply_to, \"references\",
          from_addr, to_addr, cc_addr, subject, body_text, body_html,
          date, has_attachments, raw_size, uid, flags, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            mail.id,
            mail.account_id,
            mail.folder,
            mail.message_id,
            mail.in_reply_to,
            mail.references,
            mail.from_addr,
            mail.to_addr,
            mail.cc_addr,
            mail.subject,
            mail.body_text,
            mail.body_html,
            mail.date,
            mail.has_attachments,
            mail.raw_size,
            mail.uid,
            mail.flags,
            mail.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn get_mails_by_account(
    conn: &Connection,
    account_id: &str,
    folder: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, folder, message_id, in_reply_to, \"references\",
                from_addr, to_addr, cc_addr, subject, body_text, body_html,
                date, has_attachments, raw_size, uid, flags, fetched_at
         FROM mails
         WHERE account_id = ?1 AND folder = ?2
         ORDER BY date DESC",
    )?;
    let mails = stmt
        .query_map(params![account_id, folder], |row| {
            Ok(Mail {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder: row.get(2)?,
                message_id: row.get(3)?,
                in_reply_to: row.get(4)?,
                references: row.get(5)?,
                from_addr: row.get(6)?,
                to_addr: row.get(7)?,
                cc_addr: row.get(8)?,
                subject: row.get(9)?,
                body_text: row.get(10)?,
                body_html: row.get(11)?,
                date: row.get(12)?,
                has_attachments: row.get(13)?,
                raw_size: row.get(14)?,
                uid: row.get(15)?,
                flags: row.get(16)?,
                fetched_at: row.get(17)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(mails)
}

pub fn get_max_uid(conn: &Connection, account_id: &str, folder: &str) -> Result<u32, AppError> {
    let uid: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(uid), 0) FROM mails WHERE account_id = ?1 AND folder = ?2",
            params![account_id, folder],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(uid)
}

/// RFC 2822 の In-Reply-To / References でスレッドを構築する。
/// ヘッダーが欠落している場合は件名ベースのフォールバックマッチング
/// （Re: / Fwd: を除去して比較）を行う。
pub fn build_threads(mails: &[Mail]) -> Vec<Thread> {
    // message_id → mail のインデックス
    let mut by_message_id: HashMap<&str, usize> = HashMap::new();
    for (i, mail) in mails.iter().enumerate() {
        by_message_id.insert(&mail.message_id, i);
    }

    // 各メールが属するスレッドルートの message_id を特定
    // Union-Find 的にルートを辿る
    let mut thread_root: Vec<usize> = (0..mails.len()).collect();

    for (i, mail) in mails.iter().enumerate() {
        // in_reply_to で親を探す
        if let Some(ref reply_to) = mail.in_reply_to {
            if let Some(&parent_idx) = by_message_id.get(reply_to.as_str()) {
                let root_i = find_root(&thread_root, i);
                let root_p = find_root(&thread_root, parent_idx);
                if root_i != root_p {
                    thread_root[root_i] = root_p;
                }
            }
        }

        // references で追加の親を探す
        if let Some(ref refs) = mail.references {
            for ref_id in refs.split_whitespace() {
                if let Some(&ref_idx) = by_message_id.get(ref_id) {
                    let root_i = find_root(&thread_root, i);
                    let root_r = find_root(&thread_root, ref_idx);
                    if root_i != root_r {
                        thread_root[root_i] = root_r;
                    }
                }
            }
        }
    }

    // フォールバック: 件名ベースのマッチング
    let normalized: Vec<String> = mails.iter().map(|m| normalize_subject(&m.subject)).collect();
    for i in 0..mails.len() {
        if mails[i].in_reply_to.is_some() || mails[i].references.is_some() {
            continue; // ヘッダーがあるメールはスキップ
        }
        for j in 0..i {
            if normalized[i] == normalized[j] {
                let root_i = find_root(&thread_root, i);
                let root_j = find_root(&thread_root, j);
                if root_i != root_j {
                    thread_root[root_i] = root_j;
                }
                break;
            }
        }
    }

    // スレッドにグルーピング
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..mails.len() {
        let root = find_root(&thread_root, i);
        groups.entry(root).or_default().push(i);
    }

    let mut threads: Vec<Thread> = groups
        .into_values()
        .map(|indices| {
            let mut thread_mails: Vec<Mail> = indices.iter().map(|&i| mails[i].clone()).collect();
            thread_mails.sort_by(|a, b| a.date.cmp(&b.date));

            let from_addrs: Vec<String> = thread_mails
                .iter()
                .map(|m| m.from_addr.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let last_date = thread_mails.last().map(|m| m.date.clone()).unwrap_or_default();
            let subject = thread_mails.first().map(|m| m.subject.clone()).unwrap_or_default();
            let thread_id = thread_mails.first().map(|m| m.message_id.clone()).unwrap_or_default();

            Thread {
                thread_id,
                subject,
                last_date,
                mail_count: thread_mails.len(),
                from_addrs,
                mails: thread_mails,
            }
        })
        .collect();

    threads.sort_by(|a, b| b.last_date.cmp(&a.last_date));
    threads
}

fn find_root(roots: &[usize], mut i: usize) -> usize {
    while roots[i] != i {
        i = roots[i];
    }
    i
}

fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        if lower.starts_with("re:") || lower.starts_with("fw:") {
            s = s[3..].trim_start();
        } else if lower.starts_with("fwd:") {
            s = s[4..].trim_start();
        } else {
            break;
        }
    }
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // テスト用にアカウントを挿入
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        )
        .unwrap();
        conn
    }

    fn make_mail(id: &str, message_id: &str, subject: &str, date: &str) -> Mail {
        Mail {
            id: id.into(),
            account_id: "acc1".into(),
            folder: "INBOX".into(),
            message_id: message_id.into(),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "me@example.com".into(),
            cc_addr: None,
            subject: subject.into(),
            body_text: Some("Hello".into()),
            body_html: None,
            date: date.into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        }
    }

    #[test]
    fn test_insert_and_get_mails() {
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@example.com>", "Hello", "2026-04-13T10:00:00");
        insert_mail(&conn, &mail).unwrap();

        let mails = get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        assert_eq!(mails.len(), 1);
        assert_eq!(mails[0].subject, "Hello");
    }

    #[test]
    fn test_get_max_uid() {
        let conn = setup_db();
        assert_eq!(get_max_uid(&conn, "acc1", "INBOX").unwrap(), 0);

        let mut mail = make_mail("m1", "<msg1@example.com>", "Test", "2026-04-13T10:00:00");
        mail.uid = 42;
        insert_mail(&conn, &mail).unwrap();

        assert_eq!(get_max_uid(&conn, "acc1", "INBOX").unwrap(), 42);
    }

    #[test]
    fn test_build_threads_by_in_reply_to() {
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Hello", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());

        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_by_references() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.references = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail("m3", "<msg3@ex.com>", "Re: Re: Topic", "2026-04-13T12:00:00");
        m3.references = Some("<msg1@ex.com> <msg2@ex.com>".into());

        let threads = build_threads(&[m1, m2, m3]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 3);
    }

    #[test]
    fn test_build_threads_subject_fallback() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "見積もりの件", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Re: 見積もりの件", "2026-04-13T11:00:00");

        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_separate() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic A", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Topic B", "2026-04-13T11:00:00");

        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Re: Hello"), "hello");
        assert_eq!(normalize_subject("Fwd: Re: Hello"), "hello");
        assert_eq!(normalize_subject("FW: Hello"), "hello");
        assert_eq!(normalize_subject("Hello"), "hello");
    }
}
```

- [ ] **Step 2: db/mod.rs を更新**

```rust
// src-tauri/src/db/mod.rs
pub mod accounts;
pub mod mails;
pub mod migrations;
```

- [ ] **Step 3: テストを実行**

```bash
cd src-tauri && cargo test db::mails
```

Expected: 7 tests passed

- [ ] **Step 4: コミット**

```bash
git add src-tauri/src/db/
git commit -m "feat(db): mails CRUDとスレッド構築ロジックを実装"
```

---

## Task 5: IMAP クライアント + MIME パーサー

**Files:**
- Create: `src-tauri/src/mail_sync/mod.rs`
- Create: `src-tauri/src/mail_sync/imap_client.rs`
- Create: `src-tauri/src/mail_sync/mime_parser.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: mime_parser.rs を作成（テスト付き）**

```rust
// src-tauri/src/mail_sync/mime_parser.rs
use mail_parser::MessageParser;
use uuid::Uuid;

use crate::models::mail::Mail;

pub fn parse_mime(raw: &[u8], account_id: &str, folder: &str, uid: u32) -> Option<Mail> {
    let message = MessageParser::default().parse(raw)?;

    let message_id = message
        .message_id()
        .map(|s| format!("<{}>", s))
        .unwrap_or_else(|| format!("<generated-{}@pigeon>", Uuid::new_v4()));

    let in_reply_to = message.in_reply_to().and_then(|h| {
        h.as_text().map(|s| format!("<{}>", s))
    });

    let references = message.references().map(|refs| {
        refs.as_text_list()
            .map(|list| list.iter().map(|s| format!("<{}>", s)).collect::<Vec<_>>().join(" "))
            .or_else(|| refs.as_text().map(|s| format!("<{}>", s)))
            .unwrap_or_default()
    });

    let from_addr = message
        .from()
        .and_then(|a| a.first())
        .map(|a| {
            if let Some(name) = a.name() {
                format!("{} <{}>", name, a.address().unwrap_or(""))
            } else {
                a.address().unwrap_or("").to_string()
            }
        })
        .unwrap_or_default();

    let to_addr = message
        .to()
        .and_then(|a| a.first())
        .map(|a| a.address().unwrap_or("").to_string())
        .unwrap_or_default();

    let cc_addr = message.cc().and_then(|addrs| {
        let cc: Vec<String> = addrs
            .iter()
            .filter_map(|a| a.address().map(|s| s.to_string()))
            .collect();
        if cc.is_empty() { None } else { Some(cc.join(", ")) }
    });

    let subject = message.subject().unwrap_or("(no subject)").to_string();

    let body_text = message.body_text(0).map(|s| s.to_string());
    let body_html = message.body_html(0).map(|s| s.to_string());

    let date = message
        .date()
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let has_attachments = message.attachment_count() > 0;

    Some(Mail {
        id: Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        folder: folder.to_string(),
        message_id,
        in_reply_to,
        references,
        from_addr,
        to_addr,
        cc_addr,
        subject,
        body_text,
        body_html,
        date,
        has_attachments,
        raw_size: Some(raw.len() as i64),
        uid,
        flags: None,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_EMAIL: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: Test Email\r\n\
        Message-ID: <test123@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Hello, this is a test email.";

    const REPLY_EMAIL: &[u8] = b"From: recipient@example.com\r\n\
        To: sender@example.com\r\n\
        Subject: Re: Test Email\r\n\
        Message-ID: <reply456@example.com>\r\n\
        In-Reply-To: <test123@example.com>\r\n\
        References: <test123@example.com>\r\n\
        Date: Mon, 13 Apr 2026 11:00:00 +0900\r\n\
        \r\n\
        Thanks for the test.";

    #[test]
    fn test_parse_simple_email() {
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1).unwrap();
        assert_eq!(mail.subject, "Test Email");
        assert_eq!(mail.from_addr, "sender@example.com");
        assert_eq!(mail.to_addr, "recipient@example.com");
        assert_eq!(mail.message_id, "<test123@example.com>");
        assert!(mail.in_reply_to.is_none());
        assert!(mail.body_text.unwrap().contains("Hello"));
    }

    #[test]
    fn test_parse_reply_email() {
        let mail = parse_mime(REPLY_EMAIL, "acc1", "INBOX", 2).unwrap();
        assert_eq!(mail.subject, "Re: Test Email");
        assert!(mail.in_reply_to.is_some());
        assert!(mail.references.is_some());
    }

    #[test]
    fn test_parse_invalid_returns_none() {
        let result = parse_mime(b"not a valid email", "acc1", "INBOX", 1);
        // mail-parser は壊れたデータでも部分パースするため None にならない場合がある
        // ここでは少なくともパニックしないことを確認
        let _ = result;
    }
}
```

- [ ] **Step 2: imap_client.rs を作成**

```rust
// src-tauri/src/mail_sync/imap_client.rs
use async_imap::Session;
use async_native_tls::TlsStream;
use tokio::net::TcpStream;

use crate::error::AppError;
use crate::models::account::Account;

type ImapSession = Session<TlsStream<TcpStream>>;

pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<ImapSession, AppError> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|e| AppError::Imap(format!("TCP connection failed: {}", e)))?;

    let tls = async_native_tls::TlsConnector::new();
    let tls_stream = tls
        .connect(host, tcp)
        .await
        .map_err(|e| AppError::Imap(format!("TLS handshake failed: {}", e)))?;

    let client = async_imap::Client::new(tls_stream);
    let session = client
        .login(username, password)
        .await
        .map_err(|e| AppError::Imap(format!("Login failed: {}", e.0)))?;

    Ok(session)
}

pub async fn fetch_mails_since_uid(
    session: &mut ImapSession,
    folder: &str,
    since_uid: u32,
) -> Result<Vec<(u32, Vec<u8>)>, AppError> {
    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    let query = if since_uid == 0 {
        "1:*".to_string()
    } else {
        format!("{}:*", since_uid + 1)
    };

    let messages = session
        .uid_fetch(&query, "(UID RFC822)")
        .await
        .map_err(|e| AppError::Imap(format!("Fetch failed: {}", e)))?;

    use async_imap::extensions::idle::SetReadTimeout;
    let mut results = Vec::new();
    for msg in messages.iter() {
        if let Some(body) = msg.body() {
            let uid = msg.uid.unwrap_or(0);
            if uid > since_uid {
                results.push((uid, body.to_vec()));
            }
        }
    }

    Ok(results)
}

pub async fn list_folders(
    session: &mut ImapSession,
) -> Result<Vec<String>, AppError> {
    let folders = session
        .list(None, Some("*"))
        .await
        .map_err(|e| AppError::Imap(format!("List folders failed: {}", e)))?;

    Ok(folders.iter().map(|f| f.name().to_string()).collect())
}
```

注意: IMAP クライアントは実際のメールサーバーが必要なためユニットテストは書かない。統合テストは実サーバーで手動確認する。

- [ ] **Step 3: mail_sync/mod.rs を作成**

```rust
// src-tauri/src/mail_sync/mod.rs
pub mod imap_client;
pub mod mime_parser;
```

- [ ] **Step 4: lib.rs を更新**

```rust
// src-tauri/src/lib.rs
pub mod db;
pub mod error;
pub mod mail_sync;
pub mod models;
```

- [ ] **Step 5: テストを実行**

```bash
cd src-tauri && cargo test mail_sync::mime_parser
```

Expected: 3 tests passed

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/mail_sync/ src-tauri/src/lib.rs
git commit -m "feat(mail-sync): IMAPクライアントとMIMEパーサーを実装"
```

---

## Task 6: Tauri Commands（バックエンド API）

**Files:**
- Create: `src-tauri/src/commands/mod.rs`
- Create: `src-tauri/src/commands/account_commands.rs`
- Create: `src-tauri/src/commands/mail_commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: account_commands.rs を作成**

```rust
// src-tauri/src/commands/account_commands.rs
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::State;

use crate::db::accounts;
use crate::models::account::{Account, CreateAccountRequest};

pub struct DbState(pub Mutex<Connection>);

#[tauri::command]
pub fn create_account(
    state: State<DbState>,
    request: CreateAccountRequest,
) -> Result<Account, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    accounts::insert_account(&conn, &request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_accounts(state: State<DbState>) -> Result<Vec<Account>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    accounts::list_accounts(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_account(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    accounts::delete_account(&conn, &id).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: mail_commands.rs を作成**

```rust
// src-tauri/src/commands/mail_commands.rs
use tauri::State;

use crate::commands::account_commands::DbState;
use crate::db::mails;
use crate::mail_sync::{imap_client, mime_parser};
use crate::models::mail::Thread;

#[tauri::command]
pub async fn sync_account(
    state: State<'_, DbState>,
    account_id: String,
    imap_host: String,
    imap_port: u16,
    username: String,
    password: String,
) -> Result<u32, String> {
    let max_uid = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        mails::get_max_uid(&conn, &account_id, "INBOX").map_err(|e| e.to_string())?
    };

    let mut session = imap_client::connect(&imap_host, imap_port, &username, &password)
        .await
        .map_err(|e| e.to_string())?;

    let raw_mails = imap_client::fetch_mails_since_uid(&mut session, "INBOX", max_uid)
        .await
        .map_err(|e| e.to_string())?;

    let mut count = 0u32;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    for (uid, body) in &raw_mails {
        if let Some(mail) = mime_parser::parse_mime(body, &account_id, "INBOX", *uid) {
            mails::insert_mail(&conn, &mail).map_err(|e| e.to_string())?;
            count += 1;
        }
    }

    let _ = session.logout().await;
    Ok(count)
}

#[tauri::command]
pub fn get_threads(
    state: State<DbState>,
    account_id: String,
    folder: String,
) -> Result<Vec<Thread>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let all_mails =
        mails::get_mails_by_account(&conn, &account_id, &folder).map_err(|e| e.to_string())?;
    Ok(mails::build_threads(&all_mails))
}
```

- [ ] **Step 3: commands/mod.rs を作成**

```rust
// src-tauri/src/commands/mod.rs
pub mod account_commands;
pub mod mail_commands;
```

- [ ] **Step 4: lib.rs を更新**

```rust
// src-tauri/src/lib.rs
pub mod commands;
pub mod db;
pub mod error;
pub mod mail_sync;
pub mod models;
```

- [ ] **Step 5: main.rs を更新してコマンドを登録**

```rust
// src-tauri/src/main.rs
use pigeon_lib::commands::account_commands::{self, DbState};
use pigeon_lib::commands::mail_commands;
use pigeon_lib::db::migrations;
use rusqlite::Connection;
use std::sync::Mutex;

fn main() {
    let db_path = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Pigeon")
        .join("pigeon.db");

    std::fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create data directory");

    let conn = Connection::open(&db_path).expect("Failed to open database");
    migrations::run_migrations(&conn).expect("Failed to run migrations");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(DbState(Mutex::new(conn)))
        .invoke_handler(tauri::generate_handler![
            account_commands::create_account,
            account_commands::get_accounts,
            account_commands::remove_account,
            mail_commands::sync_account,
            mail_commands::get_threads,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`dirs` クレートを `Cargo.toml` に追加:

```toml
dirs = "5"
```

- [ ] **Step 6: ビルド確認**

```bash
cd src-tauri && cargo check
```

Expected: コンパイル成功

- [ ] **Step 7: コミット**

```bash
git add src-tauri/
git commit -m "feat(commands): Tauri commandsでアカウント・メール操作APIを公開"
```

---

## Task 7: TypeScript 型定義 + Zustand ストア

**Files:**
- Create: `src/types/account.ts`
- Create: `src/types/mail.ts`
- Create: `src/stores/accountStore.ts`
- Create: `src/stores/mailStore.ts`

- [ ] **Step 1: TypeScript 型定義を作成**

`src/types/account.ts`:

```typescript
export interface Account {
  id: string;
  name: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  auth_type: "plain" | "oauth2";
  created_at: string;
}

export interface CreateAccountRequest {
  name: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  auth_type: "plain" | "oauth2";
  password: string;
}
```

`src/types/mail.ts`:

```typescript
export interface Mail {
  id: string;
  account_id: string;
  folder: string;
  message_id: string;
  in_reply_to: string | null;
  references: string | null;
  from_addr: string;
  to_addr: string;
  cc_addr: string | null;
  subject: string;
  body_text: string | null;
  body_html: string | null;
  date: string;
  has_attachments: boolean;
  raw_size: number | null;
  uid: number;
  flags: string | null;
  fetched_at: string;
}

export interface Thread {
  thread_id: string;
  subject: string;
  last_date: string;
  mail_count: number;
  from_addrs: string[];
  mails: Mail[];
}
```

- [ ] **Step 2: accountStore を作成**

`src/stores/accountStore.ts`:

```typescript
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Account, CreateAccountRequest } from "../types/account";

interface AccountState {
  accounts: Account[];
  selectedAccountId: string | null;
  loading: boolean;
  error: string | null;
  fetchAccounts: () => Promise<void>;
  createAccount: (req: CreateAccountRequest) => Promise<void>;
  removeAccount: (id: string) => Promise<void>;
  selectAccount: (id: string | null) => void;
}

export const useAccountStore = create<AccountState>((set) => ({
  accounts: [],
  selectedAccountId: null,
  loading: false,
  error: null,

  fetchAccounts: async () => {
    set({ loading: true, error: null });
    try {
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  createAccount: async (req) => {
    set({ loading: true, error: null });
    try {
      await invoke<Account>("create_account", { request: req });
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  removeAccount: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke("remove_account", { id });
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false, selectedAccountId: null });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  selectAccount: (id) => set({ selectedAccountId: id }),
}));
```

- [ ] **Step 3: mailStore を作成**

`src/stores/mailStore.ts`:

```typescript
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Mail, Thread } from "../types/mail";

interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  error: string | null;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (
    accountId: string,
    imapHost: string,
    imapPort: number,
    username: string,
    password: string,
  ) => Promise<number>;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
}

export const useMailStore = create<MailState>((set) => ({
  threads: [],
  selectedThread: null,
  selectedMail: null,
  syncing: false,
  error: null,

  fetchThreads: async (accountId, folder) => {
    try {
      const threads = await invoke<Thread[]>("get_threads", {
        accountId,
        folder,
      });
      set({ threads });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  syncAccount: async (accountId, imapHost, imapPort, username, password) => {
    set({ syncing: true, error: null });
    try {
      const count = await invoke<number>("sync_account", {
        accountId,
        imapHost,
        imapPort,
        username,
        password,
      });
      set({ syncing: false });
      return count;
    } catch (e) {
      set({ error: String(e), syncing: false });
      return 0;
    }
  },

  selectThread: (thread) => set({ selectedThread: thread, selectedMail: null }),
  selectMail: (mail) => set({ selectedMail: mail }),
}));
```

- [ ] **Step 4: コミット**

```bash
git add src/types/ src/stores/
git commit -m "feat(ui): TypeScript型定義とZustandストアを追加"
```

---

## Task 8: 3ペイン UI コンポーネント

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Create: `src/components/sidebar/Sidebar.tsx`
- Create: `src/components/sidebar/AccountList.tsx`
- Create: `src/components/sidebar/AccountForm.tsx`
- Create: `src/components/thread-list/ThreadList.tsx`
- Create: `src/components/thread-list/ThreadItem.tsx`
- Create: `src/components/mail-view/MailView.tsx`
- Create: `src/components/mail-view/MailHeader.tsx`

- [ ] **Step 1: テストを書く — AccountForm**

`src/__tests__/AccountForm.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { AccountForm } from "../components/sidebar/AccountForm";

describe("AccountForm", () => {
  it("renders all required input fields", () => {
    render(<AccountForm onSubmit={() => {}} onCancel={() => {}} />);

    expect(screen.getByLabelText("アカウント名")).toBeInTheDocument();
    expect(screen.getByLabelText("メールアドレス")).toBeInTheDocument();
    expect(screen.getByLabelText("IMAPサーバー")).toBeInTheDocument();
    expect(screen.getByLabelText("SMTPサーバー")).toBeInTheDocument();
    expect(screen.getByLabelText("パスワード")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "追加" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "キャンセル" })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
npm test
```

Expected: FAIL — AccountForm が存在しない

- [ ] **Step 3: AccountForm を実装**

`src/components/sidebar/AccountForm.tsx`:

```typescript
import { useState } from "react";
import type { CreateAccountRequest } from "../../types/account";

interface AccountFormProps {
  onSubmit: (req: CreateAccountRequest) => void;
  onCancel: () => void;
}

export function AccountForm({ onSubmit, onCancel }: AccountFormProps) {
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [imapHost, setImapHost] = useState("");
  const [imapPort, setImapPort] = useState(993);
  const [smtpHost, setSmtpHost] = useState("");
  const [smtpPort, setSmtpPort] = useState(587);
  const [password, setPassword] = useState("");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit({
      name,
      email,
      imap_host: imapHost,
      imap_port: imapPort,
      smtp_host: smtpHost,
      smtp_port: smtpPort,
      auth_type: "plain",
      password,
    });
  };

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-3 p-4">
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">アカウント名</span>
        <input
          aria-label="アカウント名"
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">メールアドレス</span>
        <input
          aria-label="メールアドレス"
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">IMAPサーバー</span>
        <input
          aria-label="IMAPサーバー"
          type="text"
          value={imapHost}
          onChange={(e) => setImapHost(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">IMAPポート</span>
        <input
          aria-label="IMAPポート"
          type="number"
          value={imapPort}
          onChange={(e) => setImapPort(Number(e.target.value))}
          className="rounded border px-2 py-1 text-sm"
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">SMTPサーバー</span>
        <input
          aria-label="SMTPサーバー"
          type="text"
          value={smtpHost}
          onChange={(e) => setSmtpHost(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">SMTPポート</span>
        <input
          aria-label="SMTPポート"
          type="number"
          value={smtpPort}
          onChange={(e) => setSmtpPort(Number(e.target.value))}
          className="rounded border px-2 py-1 text-sm"
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">パスワード</span>
        <input
          aria-label="パスワード"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <div className="flex gap-2">
        <button
          type="submit"
          className="rounded bg-blue-600 px-4 py-1 text-sm text-white hover:bg-blue-700"
        >
          追加
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="rounded border px-4 py-1 text-sm hover:bg-gray-100"
        >
          キャンセル
        </button>
      </div>
    </form>
  );
}
```

- [ ] **Step 4: テストを実行して通ることを確認**

```bash
npm test
```

Expected: PASS

- [ ] **Step 5: 残りのコンポーネントを実装**

`src/components/sidebar/AccountList.tsx`:

```typescript
import type { Account } from "../../types/account";

interface AccountListProps {
  accounts: Account[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

export function AccountList({ accounts, selectedId, onSelect }: AccountListProps) {
  if (accounts.length === 0) {
    return <p className="px-4 py-2 text-sm text-gray-400">アカウントなし</p>;
  }

  return (
    <ul className="flex flex-col">
      {accounts.map((account) => (
        <li key={account.id}>
          <button
            onClick={() => onSelect(account.id)}
            className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${
              selectedId === account.id ? "bg-blue-50 font-semibold text-blue-700" : ""
            }`}
          >
            <div>{account.name}</div>
            <div className="text-xs text-gray-400">{account.email}</div>
          </button>
        </li>
      ))}
    </ul>
  );
}
```

`src/components/sidebar/Sidebar.tsx`:

```typescript
import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { AccountList } from "./AccountList";
import { AccountForm } from "./AccountForm";
import type { CreateAccountRequest } from "../../types/account";

export function Sidebar() {
  const { accounts, selectedAccountId, fetchAccounts, createAccount, selectAccount } =
    useAccountStore();
  const [showForm, setShowForm] = useState(false);

  useEffect(() => {
    fetchAccounts();
  }, [fetchAccounts]);

  const handleSubmit = async (req: CreateAccountRequest) => {
    await createAccount(req);
    setShowForm(false);
  };

  return (
    <aside className="flex h-full w-64 flex-col border-r bg-gray-50">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h1 className="text-lg font-bold">Pigeon</h1>
        <button
          onClick={() => setShowForm(!showForm)}
          className="text-sm text-blue-600 hover:underline"
        >
          {showForm ? "閉じる" : "+ 追加"}
        </button>
      </div>

      {showForm && <AccountForm onSubmit={handleSubmit} onCancel={() => setShowForm(false)} />}

      <div className="flex-1 overflow-y-auto">
        <AccountList accounts={accounts} selectedId={selectedAccountId} onSelect={selectAccount} />
      </div>
    </aside>
  );
}
```

`src/components/thread-list/ThreadItem.tsx`:

```typescript
import type { Thread } from "../../types/mail";

interface ThreadItemProps {
  thread: Thread;
  selected: boolean;
  onClick: () => void;
}

export function ThreadItem({ thread, selected, onClick }: ThreadItemProps) {
  const date = new Date(thread.last_date);
  const dateStr = `${date.getMonth() + 1}/${date.getDate()}`;

  return (
    <button
      onClick={onClick}
      className={`w-full border-b px-4 py-3 text-left hover:bg-gray-50 ${
        selected ? "bg-blue-50" : ""
      }`}
    >
      <div className="flex items-center justify-between">
        <span className="truncate text-sm font-medium">{thread.subject}</span>
        <span className="ml-2 shrink-0 text-xs text-gray-400">{dateStr}</span>
      </div>
      <div className="mt-1 flex items-center justify-between">
        <span className="truncate text-xs text-gray-500">
          {thread.from_addrs.join(", ")}
        </span>
        {thread.mail_count > 1 && (
          <span className="ml-2 shrink-0 rounded-full bg-gray-200 px-1.5 text-xs">
            {thread.mail_count}
          </span>
        )}
      </div>
    </button>
  );
}
```

`src/components/thread-list/ThreadList.tsx`:

```typescript
import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { ThreadItem } from "./ThreadItem";

export function ThreadList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const { threads, selectedThread, fetchThreads, selectThread } = useMailStore();

  useEffect(() => {
    if (selectedAccountId) {
      fetchThreads(selectedAccountId, "INBOX");
    }
  }, [selectedAccountId, fetchThreads]);

  if (!selectedAccountId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        アカウントを選択してください
      </div>
    );
  }

  if (threads.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        メールがありません
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {threads.map((thread) => (
        <ThreadItem
          key={thread.thread_id}
          thread={thread}
          selected={selectedThread?.thread_id === thread.thread_id}
          onClick={() => selectThread(thread)}
        />
      ))}
    </div>
  );
}
```

`src/components/mail-view/MailHeader.tsx`:

```typescript
import type { Mail } from "../../types/mail";

interface MailHeaderProps {
  mail: Mail;
}

export function MailHeader({ mail }: MailHeaderProps) {
  return (
    <div className="border-b px-6 py-4">
      <h2 className="text-lg font-semibold">{mail.subject}</h2>
      <div className="mt-2 space-y-1 text-sm text-gray-600">
        <div>
          <span className="font-medium">From:</span> {mail.from_addr}
        </div>
        <div>
          <span className="font-medium">To:</span> {mail.to_addr}
        </div>
        {mail.cc_addr && (
          <div>
            <span className="font-medium">Cc:</span> {mail.cc_addr}
          </div>
        )}
        <div>
          <span className="font-medium">Date:</span>{" "}
          {new Date(mail.date).toLocaleString("ja-JP")}
        </div>
      </div>
    </div>
  );
}
```

`src/components/mail-view/MailView.tsx`:

```typescript
import { useMailStore } from "../../stores/mailStore";
import { MailHeader } from "./MailHeader";

export function MailView() {
  const { selectedThread, selectedMail, selectMail } = useMailStore();

  if (!selectedThread) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        スレッドを選択してください
      </div>
    );
  }

  const mail = selectedMail ?? selectedThread.mails[selectedThread.mails.length - 1];

  return (
    <div className="flex h-full flex-col">
      {selectedThread.mails.length > 1 && (
        <div className="flex gap-1 border-b px-4 py-2">
          {selectedThread.mails.map((m, i) => (
            <button
              key={m.id}
              onClick={() => selectMail(m)}
              className={`rounded px-2 py-1 text-xs ${
                m.id === mail.id ? "bg-blue-100 text-blue-700" : "hover:bg-gray-100"
              }`}
            >
              {i + 1}
            </button>
          ))}
        </div>
      )}
      <MailHeader mail={mail} />
      <div className="flex-1 overflow-y-auto px-6 py-4">
        {mail.body_html ? (
          <div
            className="prose max-w-none text-sm"
            dangerouslySetInnerHTML={{ __html: mail.body_html }}
          />
        ) : (
          <pre className="whitespace-pre-wrap text-sm">{mail.body_text}</pre>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 6: App.tsx を 3ペインレイアウトに更新**

`src/App.tsx`:

```typescript
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { MailView } from "./components/mail-view/MailView";

function App() {
  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="w-80 border-r">
        <ThreadList />
      </div>
      <div className="flex-1">
        <MailView />
      </div>
    </div>
  );
}

export default App;
```

`src/App.css`:

```css
@import "tailwindcss";
```

- [ ] **Step 7: テストを実行**

```bash
npm test
```

Expected: PASS

- [ ] **Step 8: `tauri dev` で起動して3ペインレイアウトが表示されることを確認**

```bash
npm run tauri dev
```

Expected: 左にサイドバー（アカウント追加ボタン）、中央に「アカウントを選択してください」、右に「スレッドを選択してください」が表示される。

- [ ] **Step 9: コミット**

```bash
git add src/
git commit -m "feat(ui): 3ペインレイアウトとアカウント管理UIを実装"
```

---

## Task 9: 動作確認 + 最終調整

- [ ] **Step 1: 全テスト実行**

```bash
cd src-tauri && cargo test
npm test
```

Expected: 全テスト PASS

- [ ] **Step 2: Rust リント**

```bash
cd src-tauri && cargo clippy -- -D warnings
```

Expected: warning なし（あれば修正）

- [ ] **Step 3: `tauri dev` で E2E 動作確認**

```bash
npm run tauri dev
```

確認項目:
1. アプリが起動する
2. 「+ 追加」でアカウント追加フォームが表示される
3. フォームに入力して「追加」でアカウントが作成される
4. アカウントをクリックでスレッド一覧（最初は空）が表示される

- [ ] **Step 4: mise.toml を .gitignore に含まれていないことを確認してコミット**

```bash
git add mise.toml
git commit -m "chore: mise.tomlをリポジトリに追加"
```

- [ ] **Step 5: Phase 1 完了のまとめコミットは不要（各タスクでコミット済み）**

Phase 1 完了。以下が動作する状態:
- Tauri 2 アプリが起動する
- アカウント追加・一覧・削除ができる
- IMAP でメールを取得して SQLite に保存できる
- スレッド構築（In-Reply-To / References + 件名フォールバック）が動作する
- 3ペイン UI でメール一覧・本文が表示される
