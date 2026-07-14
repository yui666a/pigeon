# OAuth 再認証フロー実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** OAuth トークンが失われた（Stronghold とDB の不整合）場合に、ユーザーに再認証を促すフローを追加する。

**Architecture:** バックエンドの `get_accounts` コマンドで各アカウントの認証情報の有無を検証し、`needs_reauth` フラグとして返す。フロントはこのフラグに基づき再認証バナーを表示し、再認証ボタンから既存の OAuth フローを既存アカウント ID で再実行する。`start_oauth` に既存アカウント ID を渡せるよう拡張し、コールバック時に新規作成ではなくトークン更新のみ行う分岐を追加する。

**Tech Stack:** Rust (Tauri commands), TypeScript (React + Zustand), SQLite, iota_stronghold

---

## ファイル構成

| 操作 | ファイル | 責務 |
|------|----------|------|
| Modify | `src-tauri/src/models/account.rs` | `Account` 構造体に `needs_reauth` フィールド追加 |
| Modify | `src-tauri/src/db/accounts.rs` | `needs_reauth` をクエリ外で付与するヘルパー追加 |
| Modify | `src-tauri/src/commands/account_commands.rs` | `get_accounts` で Stronghold チェックし `needs_reauth` を設定 |
| Modify | `src-tauri/src/commands/auth_commands.rs` | `start_oauth` に既存アカウント ID 対応、コールバックで reauth 分岐 |
| Modify | `src-tauri/src/error.rs` | `ReauthRequired` バリアント追加 |
| Modify | `src-tauri/src/commands/mail_commands.rs` | `sync_account` で `ReauthRequired` エラーを返す |
| Modify | `src/stores/accountStore.ts` | `startReauth` アクション追加 |
| Modify | `src/stores/mailStore.ts` | `needsReauth` 状態、reauth エラー検知 |
| Modify | `src/components/thread-list/ThreadList.tsx` | reauth 必要時のバナー表示 |
| Modify | `src/components/sidebar/AccountList.tsx` | reauth ボタン追加 |

---

### Task 1: `AppError::ReauthRequired` バリアント追加 (Rust)

**Files:**
- Modify: `src-tauri/src/error.rs`

- [ ] **Step 1: `ReauthRequired` バリアントを追加**

`src-tauri/src/error.rs` の `AppError` enum に以下を追加する（`Stronghold` の直後）:

```rust
    #[error("Reauth required: {0}")]
    ReauthRequired(String),
```

- [ ] **Step 2: テスト実行**

Run: `cd src-tauri && cargo test`
Expected: 全テスト PASS（既存コードはこのバリアントを使っていないため影響なし）

- [ ] **Step 3: コミット**

```bash
git add src-tauri/src/error.rs
git commit -m "feat(error): add ReauthRequired error variant"
```

---

### Task 2: `Account` 構造体に `needs_reauth` を追加 (Rust)

**Files:**
- Modify: `src-tauri/src/models/account.rs`
- Modify: `src-tauri/src/db/accounts.rs`

- [ ] **Step 1: `Account` 構造体にフィールド追加**

`src-tauri/src/models/account.rs` の `Account` 構造体に `needs_reauth` を追加する。これは DB カラムではなく、API レスポンス時に動的に設定するフィールド。

```rust
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
    pub provider: AccountProvider,
    pub created_at: String,
    #[serde(default)]
    pub needs_reauth: bool,
}
```

- [ ] **Step 2: DB クエリ関数の修正**

`src-tauri/src/db/accounts.rs` の `get_account`、`list_accounts`、`account_exists_by_email` の各関数で `Account` を構築している箇所に `needs_reauth: false` を追加する。

`get_account` の例:
```rust
Ok(Account {
    id: row.get(0)?,
    name: row.get(1)?,
    email: row.get(2)?,
    imap_host: row.get(3)?,
    imap_port: row.get::<_, u32>(4)? as u16,
    smtp_host: row.get(5)?,
    smtp_port: row.get::<_, u32>(6)? as u16,
    auth_type: AuthType::try_from(auth_str.as_str()).unwrap_or(AuthType::Plain),
    provider: AccountProvider::try_from(provider_str.as_str()).unwrap_or(AccountProvider::Other),
    created_at: row.get(9)?,
    needs_reauth: false,
})
```

同じパターンを `list_accounts` と `account_exists_by_email` にも適用する。

- [ ] **Step 3: テスト実行**

Run: `cd src-tauri && cargo test`
Expected: 全テスト PASS

- [ ] **Step 4: コミット**

```bash
git add src-tauri/src/models/account.rs src-tauri/src/db/accounts.rs
git commit -m "feat(model): add needs_reauth field to Account struct"
```

---

### Task 3: `get_accounts` でトークン有無チェック (Rust)

**Files:**
- Modify: `src-tauri/src/commands/account_commands.rs`

- [ ] **Step 1: テストを書く**

`src-tauri/src/commands/account_commands.rs` にテストモジュールを追加する。`get_accounts` の内部ロジックをテスト可能にするため、`check_accounts_reauth` ヘルパーを先にテストする。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::account::{AccountProvider, AuthType};

    fn make_account(id: &str, provider: AccountProvider) -> Account {
        Account {
            id: id.to_string(),
            name: "Test".to_string(),
            email: "test@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            auth_type: if provider == AccountProvider::Google {
                AuthType::Oauth2
            } else {
                AuthType::Plain
            },
            provider,
            created_at: "2026-01-01".to_string(),
            needs_reauth: false,
        }
    }

    #[test]
    fn test_check_reauth_marks_google_without_token() {
        // SecureStore に何も保存されていない状態
        let key = sha2::Sha256::digest(b"test-key");
        let dir = tempfile::tempdir().unwrap();
        let store = crate::secure_store::SecureStore::new(dir.path().join("test.stronghold"), &key)
            .unwrap();

        let mut accounts = vec![
            make_account("acc-google", AccountProvider::Google),
            make_account("acc-other", AccountProvider::Other),
        ];

        check_accounts_reauth(&mut accounts, &store);

        assert!(accounts[0].needs_reauth, "Google account without token should need reauth");
        assert!(!accounts[1].needs_reauth, "Non-OAuth account should not need reauth");
    }

    #[test]
    fn test_check_reauth_does_not_mark_google_with_token() {
        let key = sha2::Sha256::digest(b"test-key");
        let dir = tempfile::tempdir().unwrap();
        let store = crate::secure_store::SecureStore::new(dir.path().join("test.stronghold"), &key)
            .unwrap();

        // OAuth トークンを保存
        let token_data = crate::models::account::OAuthTokenData {
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            expires_at: 9999999999,
            email: "test@gmail.com".to_string(),
        };
        crate::commands::auth_commands::save_oauth_token(&store, "acc-google", &token_data).unwrap();

        let mut accounts = vec![make_account("acc-google", AccountProvider::Google)];
        check_accounts_reauth(&mut accounts, &store);

        assert!(!accounts[0].needs_reauth, "Google account with token should not need reauth");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib commands::account_commands`
Expected: FAIL（`check_accounts_reauth` が未定義）

- [ ] **Step 3: `check_accounts_reauth` 関数を実装**

`src-tauri/src/commands/account_commands.rs` に以下を追加:

```rust
use crate::models::account::AccountProvider;
use crate::secure_store::SecureStore;

fn check_accounts_reauth(accounts: &mut [Account], secure_store: &SecureStore) {
    for account in accounts.iter_mut() {
        if account.provider == AccountProvider::Google {
            let key = format!("oauth_{}", account.id);
            match secure_store.get(&key) {
                Ok(Some(_)) => {}
                _ => {
                    account.needs_reauth = true;
                }
            }
        }
    }
}
```

- [ ] **Step 4: `get_accounts` コマンドでチェックを呼び出す**

`get_accounts` を修正して `SecureStore` を受け取り、チェックを実行する:

```rust
#[tauri::command]
pub fn get_accounts(
    state: State<DbState>,
    secure_store: State<SecureStoreState>,
) -> Result<Vec<Account>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    let mut accounts = accounts::list_accounts(&conn)?;
    check_accounts_reauth(&mut accounts, &secure_store.0);
    Ok(accounts)
}
```

- [ ] **Step 5: テスト実行**

Run: `cd src-tauri && cargo test`
Expected: 全テスト PASS

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/commands/account_commands.rs
git commit -m "feat(account): check OAuth token presence on get_accounts"
```

---

### Task 4: `sync_account` で `ReauthRequired` を返す (Rust)

**Files:**
- Modify: `src-tauri/src/commands/mail_commands.rs`

- [ ] **Step 1: `resolve_imap_credentials` のエラーを `ReauthRequired` に変更**

`src-tauri/src/commands/mail_commands.rs` の `resolve_imap_credentials` 関数で、`load_oauth_token` が失敗した場合に `ReauthRequired` を返すよう修正する:

```rust
async fn resolve_imap_credentials(
    account: &Account,
    secure_store: &crate::secure_store::SecureStore,
) -> Result<(AuthType, String, String), AppError> {
    match account.provider {
        AccountProvider::Google => {
            let mut token_data =
                match crate::commands::auth_commands::load_oauth_token(secure_store, &account.id) {
                    Ok(data) => data,
                    Err(_) => {
                        return Err(AppError::ReauthRequired(account.id.clone()));
                    }
                };

            if oauth::token_needs_refresh(&token_data) {
                let config = oauth::OAuthConfig::google()?;
                match oauth::refresh_token(&config, &token_data.refresh_token).await {
                    Ok(response) => {
                        token_data = oauth::build_token_data(
                            &response,
                            &token_data.email,
                            Some(&token_data.refresh_token),
                        )?;
                        crate::commands::auth_commands::save_oauth_token(
                            secure_store,
                            &account.id,
                            &token_data,
                        )?;
                    }
                    Err(_) => {
                        return Err(AppError::ReauthRequired(account.id.clone()));
                    }
                }
            }

            Ok((AuthType::Oauth2, token_data.email, token_data.access_token))
        }
        AccountProvider::Other => {
            let password =
                crate::commands::auth_commands::load_password(secure_store, &account.id)?;
            Ok((AuthType::Plain, account.email.clone(), password))
        }
    }
}
```

- [ ] **Step 2: テスト実行**

Run: `cd src-tauri && cargo test`
Expected: 全テスト PASS

- [ ] **Step 3: コミット**

```bash
git add src-tauri/src/commands/mail_commands.rs
git commit -m "fix(sync): return ReauthRequired instead of Stronghold error on missing token"
```

---

### Task 5: `start_oauth` に既存アカウント再認証対応 (Rust)

**Files:**
- Modify: `src-tauri/src/commands/auth_commands.rs`

- [ ] **Step 1: テストを書く**

`src-tauri/src/commands/auth_commands.rs` のテストモジュールに追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_sync::oauth::OAuthStateStore;

    #[test]
    fn test_start_oauth_inner_reauth_uses_existing_account_id() {
        // OAuthStateStore に保存される PendingOAuth の account_id が
        // reauth 時は渡された既存 ID になることを検証
        let oauth_store = OAuthStateStore::new();

        // start_oauth_inner は TcpListener を使うため直接テストが難しい。
        // 代わりに store_pending_oauth ヘルパーのロジックをテストする。
        let account_id = Some("existing-id-123".to_string());
        let resolved = account_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        assert_eq!(resolved, "existing-id-123");

        let account_id: Option<String> = None;
        let resolved = account_id.unwrap_or_else(|| "new-uuid".to_string());
        assert_eq!(resolved, "new-uuid");
    }
}
```

- [ ] **Step 2: `start_oauth` コマンドに `account_id` パラメータを追加**

```rust
#[tauri::command]
pub async fn start_oauth(
    app_handle: AppHandle,
    oauth_store: State<'_, OAuthStateStore>,
    provider: String,
    account_id: Option<String>,
) -> Result<String, AppError> {
    start_oauth_inner(&app_handle, &oauth_store, &provider, account_id)
}
```

- [ ] **Step 3: `start_oauth_inner` を修正**

`account_id` パラメータを受け取り、`None` の場合のみ新規 UUID を生成する:

```rust
fn start_oauth_inner(
    app_handle: &AppHandle,
    oauth_store: &OAuthStateStore,
    provider: &str,
    existing_account_id: Option<String>,
) -> Result<String, AppError> {
    match provider {
        "google" => {
            let redirect_uri = start_loopback_callback_listener(app_handle.clone())?;
            let config = OAuthConfig::google_with_redirect(redirect_uri.clone())?;
            let account_id = existing_account_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            // ... 残りは同じ
```

- [ ] **Step 4: `handle_oauth_callback_inner` で reauth を処理**

コールバック処理で、アカウントが既に存在する場合はトークン保存のみ行い、DB 挿入をスキップする:

```rust
async fn handle_oauth_callback_inner(
    db_state: &DbState,
    secure_store: &SecureStore,
    oauth_store: &OAuthStateStore,
    url: &str,
) -> Result<String, AppError> {
    let (code, state_param) = oauth::parse_callback_url(url)?;

    let pending = oauth_store
        .take(&state_param)
        .ok_or(AppError::InvalidOAuthState)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    if now - pending.created_at > 600 {
        return Err(AppError::OAuthTimeout);
    }

    let config = OAuthConfig::google_with_redirect(pending.redirect_uri.clone())?;
    let token_response = oauth::exchange_code(&config, &code, &pending.code_verifier).await?;

    let email = match &token_response.id_token {
        Some(id_token) => oauth::decode_id_token_email(id_token)?,
        None => return Err(AppError::OAuth("No ID token in response".into())),
    };

    let token_data = oauth::build_token_data(&token_response, &email, None)?;

    // Check if this is a reauth (account already exists in DB)
    let is_reauth = {
        let conn = db_state.0.lock().map_err(AppError::lock_err)?;
        accounts::get_account(&conn, &pending.account_id).is_ok()
    };

    if is_reauth {
        // Reauth: only save token, skip DB insert
        save_oauth_token(secure_store, &pending.account_id, &token_data)?;
        return Ok(pending.account_id);
    }

    // New account: check duplicate, save token, insert DB
    {
        let conn = db_state.0.lock().map_err(AppError::lock_err)?;
        if let Some(existing) = accounts::account_exists_by_email(&conn, &email)? {
            return Err(AppError::DuplicateAccount(format!(
                "Account with email {} already exists (id: {})",
                email, existing.id
            )));
        }
    }

    save_oauth_token(secure_store, &pending.account_id, &token_data)?;

    let account_result = {
        let conn = db_state.0.lock().map_err(AppError::lock_err)?;
        let req = CreateAccountRequest {
            name: email.clone(),
            email: email.clone(),
            imap_host: oauth::GOOGLE_IMAP_HOST.into(),
            imap_port: oauth::GOOGLE_IMAP_PORT,
            smtp_host: oauth::GOOGLE_SMTP_HOST.into(),
            smtp_port: oauth::GOOGLE_SMTP_PORT,
            auth_type: AuthType::Oauth2,
            provider: AccountProvider::Google,
            password: None,
        };
        accounts::insert_account_with_id(&conn, &pending.account_id, &req)
    };

    match account_result {
        Ok(account) => Ok(account.id),
        Err(e) => {
            let _ = secure_store.delete(&format!("oauth_{}", pending.account_id));
            Err(e)
        }
    }
}
```

- [ ] **Step 5: テスト実行**

Run: `cd src-tauri && cargo test`
Expected: 全テスト PASS

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/commands/auth_commands.rs
git commit -m "feat(auth): support reauth flow in start_oauth and handle_oauth_callback"
```

---

### Task 6: フロント `accountStore` に `startReauth` 追加

**Files:**
- Modify: `src/stores/accountStore.ts`

- [ ] **Step 1: `startReauth` アクションを追加**

`src/stores/accountStore.ts` の interface に `startReauth` を追加:

```typescript
interface AccountState {
  accounts: Account[];
  selectedAccountId: string | null;
  loading: boolean;
  error: string | null;
  oauthStatus: OAuthStatus;
  oauthError: string | null;
  reauthAccountId: string | null;
  fetchAccounts: () => Promise<void>;
  createAccount: (req: CreateAccountRequest) => Promise<void>;
  removeAccount: (id: string) => Promise<void>;
  selectAccount: (id: string | null) => void;
  startOAuth: (provider: string) => Promise<void>;
  startReauth: (accountId: string) => Promise<void>;
  handleOAuthCallback: (url: string) => Promise<void>;
  resetOAuth: () => void;
  initDeepLinkListener: () => Promise<() => void>;
}
```

ストアの初期値に `reauthAccountId: null` を追加。

`startReauth` の実装:

```typescript
  startReauth: async (accountId) => {
    set({ oauthStatus: "waiting", oauthError: null, reauthAccountId: accountId });
    try {
      const authUrl = await invoke<string>("start_oauth", {
        provider: "google",
        accountId,
      });
      await openUrl(authUrl);
    } catch (e) {
      set({ oauthStatus: "error", oauthError: String(e), reauthAccountId: null });
      useErrorStore.getState().addError(String(e));
    }
  },
```

`handleOAuthCallback` の修正 — 成功時に `reauthAccountId` をリセット:

```typescript
  handleOAuthCallback: async (url) => {
    if (get().oauthStatus === "exchanging") return;
    set({ oauthStatus: "exchanging" });
    try {
      await invoke("handle_oauth_callback", { url });
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, oauthStatus: "idle", oauthError: null, reauthAccountId: null });
    } catch (e) {
      set({ oauthStatus: "error", oauthError: String(e), reauthAccountId: null });
      useErrorStore.getState().addError(String(e));
    }
  },
```

`resetOAuth` の修正:

```typescript
  resetOAuth: () => {
    set({ oauthStatus: "idle", oauthError: null, reauthAccountId: null });
  },
```

- [ ] **Step 2: テスト実行**

Run: `pnpm test -- --run`
Expected: 全テスト PASS

- [ ] **Step 3: コミット**

```bash
git add src/stores/accountStore.ts
git commit -m "feat(store): add startReauth action to accountStore"
```

---

### Task 7: フロント `mailStore` で reauth エラーを検知

**Files:**
- Modify: `src/stores/mailStore.ts`

- [ ] **Step 1: `syncAccount` で reauth エラーを検知して `needsReauth` を設定**

```typescript
interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  needsReauth: boolean;
  unclassifiedMails: Mail[];
  error: string | null;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
  setThreads: (threads: Thread[]) => void;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
  fetchUnclassified: (accountId: string) => Promise<void>;
  moveMail: (mailId: string, projectId: string) => Promise<void>;
  removeUnclassifiedMail: (mailId: string) => void;
}
```

初期値に `needsReauth: false` を追加。

`syncAccount` を修正:

```typescript
  syncAccount: async (accountId) => {
    set({ syncing: true, error: null, needsReauth: false });
    try {
      const count = await invoke<number>("sync_account", { accountId });
      set({ syncing: false });
      return count;
    } catch (e) {
      const errorMsg = String(e);
      const isReauth = errorMsg.includes("Reauth required");
      set({ error: errorMsg, syncing: false, needsReauth: isReauth });
      if (!isReauth) {
        useErrorStore.getState().addError(errorMsg);
      }
      return 0;
    }
  },
```

- [ ] **Step 2: テストを追加**

`src/__tests__/stores/mailStore.test.ts` に以下を追加:

```typescript
    it("sets needsReauth on reauth error", async () => {
      mockInvoke.mockRejectedValue("Reauth required: acc1");

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(0);
      expect(useMailStore.getState().needsReauth).toBe(true);
      expect(useMailStore.getState().syncing).toBe(false);
    });

    it("does not set needsReauth on other errors", async () => {
      mockInvoke.mockRejectedValue("IMAP connection failed");

      await useMailStore.getState().syncAccount("acc1");

      expect(useMailStore.getState().needsReauth).toBe(false);
    });
```

- [ ] **Step 3: テスト実行**

Run: `pnpm test -- --run`
Expected: 全テスト PASS

- [ ] **Step 4: コミット**

```bash
git add src/stores/mailStore.ts src/__tests__/stores/mailStore.test.ts
git commit -m "feat(store): detect ReauthRequired in mailStore syncAccount"
```

---

### Task 8: `ThreadList` に再認証バナー表示

**Files:**
- Modify: `src/components/thread-list/ThreadList.tsx`

- [ ] **Step 1: reauth 状態の場合にバナーを表示**

```tsx
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { ThreadItem } from "./ThreadItem";
import { EmptyState } from "../common/EmptyState";
import type { Thread } from "../../types/mail";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const startReauth = useAccountStore((s) => s.startReauth);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const { threads, syncing, needsReauth, selectedThread, fetchThreads, syncAccount, selectThread, setThreads } =
    useMailStore();

  useEffect(() => {
    if (viewMode === "project" && selectedProjectId) {
      invoke<Thread[]>("get_threads_by_project", { projectId: selectedProjectId })
        .then((projectThreads) => {
          setThreads(projectThreads);
        })
        .catch(() => {
          setThreads([]);
        });
    } else if (viewMode === "threads" && selectedAccountId) {
      syncAccount(selectedAccountId).then(() => {
        fetchThreads(selectedAccountId, "INBOX");
      });
    }
  }, [viewMode, selectedAccountId, selectedProjectId, fetchThreads, syncAccount, setThreads]);

  if (!selectedAccountId) {
    return <EmptyState message="アカウントを選択してください" />;
  }
  if (needsReauth && selectedAccountId) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-4">
        <p className="text-sm text-amber-600">
          認証の有効期限が切れました。再ログインしてください。
        </p>
        <button
          onClick={() => startReauth(selectedAccountId)}
          className="rounded bg-blue-600 px-4 py-2 text-sm text-white hover:bg-blue-700"
        >
          再ログイン
        </button>
      </div>
    );
  }
  if (syncing) {
    return <EmptyState message="メールを同期中..." />;
  }
  if (threads.length === 0) {
    return <EmptyState message="メールがありません" />;
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

- [ ] **Step 2: テスト実行**

Run: `pnpm test -- --run`
Expected: 全テスト PASS

- [ ] **Step 3: コミット**

```bash
git add src/components/thread-list/ThreadList.tsx
git commit -m "feat(ui): show reauth banner in ThreadList when token is missing"
```

---

### Task 9: `AccountList` に再認証ボタン追加

**Files:**
- Modify: `src/components/sidebar/AccountList.tsx`
- Modify: `src/__tests__/AccountList.test.tsx`

- [ ] **Step 1: テストを書く**

`src/__tests__/AccountList.test.tsx` に以下を追加:

```tsx
  it("calls onReauth when reauth button is clicked", () => {
    const onReauth = vi.fn();
    const reauthAccount: Account = {
      ...baseAccount,
      id: "4",
      provider: "google",
      auth_type: "oauth2",
      needs_reauth: true,
    };
    render(
      <AccountList
        accounts={[reauthAccount]}
        selectedId={null}
        onSelect={() => {}}
        onRemove={() => {}}
        onReauth={onReauth}
      />,
    );
    fireEvent.click(screen.getByTitle("再認証"));
    expect(onReauth).toHaveBeenCalledWith("4");
  });
```

ファイル先頭の import に `fireEvent, vi` を追加:
```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test -- --run`
Expected: FAIL（`onReauth` prop が未定義）

- [ ] **Step 3: `AccountList` コンポーネントを修正**

`src/components/sidebar/AccountList.tsx`:

```tsx
import type { Account } from "../../types/account";

interface AccountListProps {
  accounts: Account[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onReauth?: (id: string) => void;
}

export function AccountList({
  accounts,
  selectedId,
  onSelect,
  onRemove,
  onReauth,
}: AccountListProps) {
  if (accounts.length === 0) {
    return <p className="px-4 py-2 text-sm text-gray-400">アカウントなし</p>;
  }
  return (
    <ul className="flex flex-col">
      {accounts.map((account) => (
        <li key={account.id}>
          <div
            className={`flex items-center px-4 py-2 hover:bg-gray-100 ${selectedId === account.id ? "bg-blue-50" : ""}`}
          >
            <button
              onClick={() => onSelect(account.id)}
              className={`flex-1 text-left text-sm ${selectedId === account.id ? "font-semibold text-blue-700" : ""}`}
            >
              <div className="flex items-center gap-1.5">
                {account.provider === "google" && (
                  <span
                    className="text-xs font-bold text-blue-600"
                    title="Google"
                  >
                    G
                  </span>
                )}
                <span>{account.name}</span>
                {account.needs_reauth && (
                  <span
                    className="text-xs text-amber-500"
                    title="再認証が必要です"
                  >
                    !
                  </span>
                )}
              </div>
              <div className="text-xs text-gray-400">{account.email}</div>
            </button>
            {account.needs_reauth && onReauth && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onReauth(account.id);
                }}
                className="ml-1 shrink-0 rounded px-2 py-1 text-xs text-amber-600 hover:bg-amber-50"
                title="再認証"
              >
                再認証
              </button>
            )}
            <button
              onClick={(e) => {
                e.stopPropagation();
                onRemove(account.id);
              }}
              className="ml-1 shrink-0 rounded p-1 text-gray-300 hover:bg-red-50 hover:text-red-500"
              title="アカウントを削除"
            >
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" className="h-3.5 w-3.5">
                <path fillRule="evenodd" d="M8.75 1A2.75 2.75 0 006 3.75v.443c-.795.077-1.584.176-2.365.298a.75.75 0 10.23 1.482l.149-.022.841 10.518A2.75 2.75 0 007.596 19h4.807a2.75 2.75 0 002.742-2.53l.841-10.519.149.023a.75.75 0 00.23-1.482A41.03 41.03 0 0014 4.193V3.75A2.75 2.75 0 0011.25 1h-2.5zM10 4c.84 0 1.673.025 2.5.075V3.75c0-.69-.56-1.25-1.25-1.25h-2.5c-.69 0-1.25.56-1.25 1.25v.325C8.327 4.025 9.16 4 10 4zM8.58 7.72a.75.75 0 00-1.5.06l.3 7.5a.75.75 0 101.5-.06l-.3-7.5zm4.34.06a.75.75 0 10-1.5-.06l-.3 7.5a.75.75 0 101.5.06l.3-7.5z" clipRule="evenodd" />
              </svg>
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}
```

- [ ] **Step 4: `Sidebar` から `onReauth` を渡す**

`src/components/sidebar/Sidebar.tsx` で `startReauth` を `AccountList` に渡す:

```tsx
const { startReauth } = useAccountStore();
```

```tsx
<AccountList
  accounts={accounts}
  selectedId={selectedAccountId}
  onSelect={handleSelectAccount}
  onRemove={removeAccount}
  onReauth={startReauth}
/>
```

- [ ] **Step 5: テスト実行**

Run: `pnpm test -- --run`
Expected: 全テスト PASS

- [ ] **Step 6: コミット**

```bash
git add src/components/sidebar/AccountList.tsx src/components/sidebar/Sidebar.tsx src/__tests__/AccountList.test.tsx
git commit -m "feat(ui): add reauth button to AccountList"
```

---

### Task 10: 全体結合テスト

- [ ] **Step 1: Rust テスト実行**

Run: `cd src-tauri && cargo test`
Expected: 全テスト PASS

- [ ] **Step 2: フロントテスト実行**

Run: `pnpm test -- --run`
Expected: 全テスト PASS

- [ ] **Step 3: ビルド確認**

Run: `cd src-tauri && cargo build`
Expected: ビルド成功

Run: `pnpm build`
Expected: ビルド成功

- [ ] **Step 4: コミット（必要な場合のみ）**

最終的な修正がある場合のみコミットする。
