//! アカウント参照の use case。UI / CLI / MCP のどの driver からも
//! 同じ dispatch バス（ADR 0004）を通す。
//!
//! CLI は `account_id`（UUID）を引数に取るコマンドが多く、その UUID を
//! 調べる手段がここにしか無い。読み取りのみなので Risk は Read。

use serde::Deserialize;

use crate::context::Ctx;
use crate::db::accounts;
use crate::error::AppError;
use crate::models::account::{Account, AccountProvider};
use crate::secure_store::SecureStore;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetAccountsInput {}

/// 登録済みアカウント一覧（読み取り）。
/// OAuth アカウントは SecureStore にトークンが無ければ `needs_reauth` を立てる。
pub struct GetAccountsUseCase;

#[async_trait::async_trait]
impl UseCase for GetAccountsUseCase {
    type Input = GetAccountsInput;
    type Output = Vec<Account>;
    fn name(&self) -> &'static str {
        "get_accounts"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }
    async fn run(&self, _input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let mut accounts = ctx.with_conn(accounts::list_accounts)?;
        check_accounts_reauth(&mut accounts, ctx.secure_store()?);
        Ok(accounts)
    }
}

/// OAuth トークンが SecureStore から消えているアカウントに再認証フラグを立てる。
/// SecureStore の読み取り自体が失敗した場合は警告に留める——一時的な失敗で
/// 「再認証が必要」と誤表示するより、フラグを立てない方が害が小さい。
pub(crate) fn check_accounts_reauth(accounts: &mut [Account], secure_store: &SecureStore) {
    for account in accounts.iter_mut() {
        if account.provider == AccountProvider::Google {
            let key = format!("oauth_{}", account.id);
            match secure_store.get(&key) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    account.needs_reauth = true;
                }
                Err(e) => {
                    eprintln!(
                        "[warn] Failed to check OAuth token for account {}: {}",
                        account.id, e
                    );
                }
            }
        }
    }
}

pub fn register_account_cases(registry: &mut Registry) {
    registry.register(GetAccountsUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::models::account::{AuthType, CreateAccountRequest};
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    fn request(name: &str, email: &str, provider: AccountProvider) -> CreateAccountRequest {
        let auth_type = if provider == AccountProvider::Google {
            AuthType::Oauth2
        } else {
            AuthType::Plain
        };
        CreateAccountRequest {
            name: name.to_string(),
            email: email.to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            auth_type,
            provider,
            password: None,
        }
    }

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

    #[tokio::test]
    async fn test_get_accounts_usecase_is_read_and_lists_accounts() {
        let (db, pending, batches, locks) = build_states();
        // setup_db が acc1 を先に入れているので、別 id で足す。
        db.with_conn(|conn| {
            crate::db::accounts::insert_account_with_id(
                conn,
                "acc2",
                &request("Work", "work@example.com", AccountProvider::Other),
            )?;
            Ok(())
        })
        .expect("seed");
        let store = SecureStore::in_memory();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks)
            .with_secure_store(&store);

        let input = GetAccountsInput {};
        assert_eq!(
            GetAccountsUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Read
        );
        let out = GetAccountsUseCase.run(input, &ctx).await.expect("run");
        let added = out
            .iter()
            .find(|a| a.id == "acc2")
            .expect("追加したアカウントが一覧に出る");
        assert_eq!(added.email, "work@example.com");
    }

    #[tokio::test]
    async fn test_get_accounts_usecase_marks_google_without_token() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            crate::db::accounts::insert_account_with_id(
                conn,
                "acc-google",
                &request("G", "g@gmail.com", AccountProvider::Google),
            )?;
            Ok(())
        })
        .expect("seed");
        let store = SecureStore::in_memory();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks)
            .with_secure_store(&store);

        let out = GetAccountsUseCase
            .run(GetAccountsInput {}, &ctx)
            .await
            .expect("run");
        let google = out
            .iter()
            .find(|a| a.id == "acc-google")
            .expect("google アカウントが一覧に出る");
        assert!(
            google.needs_reauth,
            "トークンが無い Google アカウントは再認証が必要"
        );
    }

    #[tokio::test]
    async fn test_get_accounts_usecase_is_registered_on_the_bus() {
        let (db, pending, batches, locks) = build_states();
        let store = SecureStore::in_memory();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks)
            .with_secure_store(&store);
        let mut registry = Registry::new();
        crate::usecase::cases::register_all(&mut registry);

        let out = crate::usecase::dispatch(&registry, "get_accounts", serde_json::json!({}), &ctx)
            .await
            .expect("dispatch get_accounts");
        assert!(out.is_array());
    }

    #[test]
    fn test_check_reauth_marks_google_without_token() {
        let store = SecureStore::in_memory();

        let mut accounts = vec![
            make_account("acc-google", AccountProvider::Google),
            make_account("acc-other", AccountProvider::Other),
        ];

        check_accounts_reauth(&mut accounts, &store);

        assert!(
            accounts[0].needs_reauth,
            "Google account without token should need reauth"
        );
        assert!(
            !accounts[1].needs_reauth,
            "Non-OAuth account should not need reauth"
        );
    }

    #[test]
    fn test_check_reauth_does_not_mark_google_with_token() {
        let store = SecureStore::in_memory();

        let token_data = crate::models::account::OAuthTokenData {
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            expires_at: 9999999999,
            email: "test@gmail.com".to_string(),
        };
        crate::commands::auth_commands::save_oauth_token(&store, "acc-google", &token_data)
            .expect("save token");

        let mut accounts = vec![make_account("acc-google", AccountProvider::Google)];
        check_accounts_reauth(&mut accounts, &store);

        assert!(
            !accounts[0].needs_reauth,
            "Google account with token should not need reauth"
        );
    }
}
