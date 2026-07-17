use tauri::{AppHandle, State};

use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::idle;
use crate::models::account::{Account, AccountProvider, AuthType, CreateAccountRequest};
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState};

#[tauri::command]
pub fn create_account(
    app: AppHandle,
    state: State<DbState>,
    secure_store: State<SecureStoreState>,
    request: CreateAccountRequest,
) -> Result<Account, AppError> {
    let account = state.with_conn(|conn| accounts::insert_account(conn, &request))?;

    // For PLAIN auth, save password to SecureStore
    if matches!(request.auth_type, AuthType::Plain) {
        if let Some(ref password) = request.password {
            crate::commands::auth_commands::save_password(&secure_store.0, &account.id, password)?;
        }
    }

    // 追加したアカウントの IDLE 監視を開始する
    idle::start_watching(&app, &account.id);

    Ok(account)
}

#[tauri::command]
pub fn get_accounts(
    state: State<DbState>,
    secure_store: State<SecureStoreState>,
) -> Result<Vec<Account>, AppError> {
    let mut accounts = state.with_conn(accounts::list_accounts)?;
    check_accounts_reauth(&mut accounts, &secure_store.0);
    Ok(accounts)
}

fn check_accounts_reauth(accounts: &mut [Account], secure_store: &SecureStore) {
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

#[tauri::command]
pub fn remove_account(app: AppHandle, state: State<DbState>, id: String) -> Result<(), AppError> {
    // 削除するアカウントの IDLE 監視を停止する
    idle::stop_watching(&app, &id);
    state.with_conn(|conn| accounts::delete_account(conn, &id))
}

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
        let store = crate::secure_store::SecureStore::in_memory();

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
        let store = crate::secure_store::SecureStore::in_memory();

        let token_data = crate::models::account::OAuthTokenData {
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            expires_at: 9999999999,
            email: "test@gmail.com".to_string(),
        };
        crate::commands::auth_commands::save_oauth_token(&store, "acc-google", &token_data)
            .unwrap();

        let mut accounts = vec![make_account("acc-google", AccountProvider::Google)];
        check_accounts_reauth(&mut accounts, &store);

        assert!(
            !accounts[0].needs_reauth,
            "Google account with token should not need reauth"
        );
    }
}
