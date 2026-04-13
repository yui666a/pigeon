use tauri::State;

use crate::commands::account_commands::DbState;
use crate::commands::auth_commands::SecureStoreState;
use crate::db::{accounts, mails};
use crate::error::AppError;
use crate::mail_sync::{imap_client, mime_parser, oauth};
use crate::models::account::{AccountProvider, AuthType};
use crate::models::mail::Thread;

#[tauri::command]
pub async fn sync_account(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
) -> Result<u32, String> {
    eprintln!("[sync_account] command called with account_id={}", account_id);
    let result = sync_account_inner(&state, &secure_store.0, &account_id).await;
    match &result {
        Ok(count) => eprintln!("[sync_account] success: {} mails", count),
        Err(e) => eprintln!("[sync_account] error: {}", e),
    }
    result.map_err(|e| e.to_string())
}

async fn sync_account_inner(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
) -> Result<u32, AppError> {
    eprintln!("[sync] starting sync for account {}", account_id);

    // Load account info from DB
    let account = {
        let conn = state.0.lock().map_err(|e| AppError::OAuth(e.to_string()))?;
        accounts::get_account(&conn, account_id)?
    };
    eprintln!("[sync] account loaded: {} ({})", account.email, account.provider.as_str());

    let max_uid = {
        let conn = state.0.lock().map_err(|e| AppError::OAuth(e.to_string()))?;
        mails::get_max_uid(&conn, account_id, "INBOX")?
    };
    eprintln!("[sync] max_uid = {}", max_uid);

    // Connect based on provider/auth_type
    let mut session = match account.provider {
        AccountProvider::Google => {
            let mut token_data =
                crate::commands::auth_commands::load_oauth_token(secure_store, account_id)?;

            // Refresh token if needed
            if oauth::token_needs_refresh(&token_data) {
                let config = oauth::OAuthConfig::google()?;
                let response = oauth::refresh_token(&config, &token_data.refresh_token).await?;
                token_data = oauth::build_token_data(
                    &response,
                    &token_data.email,
                    Some(&token_data.refresh_token),
                )?;
                // Save refreshed token
                crate::commands::auth_commands::save_oauth_token_public(
                    secure_store,
                    account_id,
                    &token_data,
                )?;
            }

            eprintln!("[sync] connecting to IMAP {}:{} with OAuth2...", account.imap_host, account.imap_port);
            let session = imap_client::connect(
                &account.imap_host,
                account.imap_port,
                &AuthType::Oauth2,
                &token_data.email,
                &token_data.access_token,
            )
            .await?;
            eprintln!("[sync] IMAP connected!");
            session
        }
        AccountProvider::Other => {
            let password = crate::commands::auth_commands::load_password(secure_store, account_id)?;
            imap_client::connect(
                &account.imap_host,
                account.imap_port,
                &AuthType::Plain,
                &account.email,
                &password,
            )
            .await?
        }
    };
    eprintln!("[sync] IMAP connected successfully");

    eprintln!("[sync] fetching mails since uid {}", max_uid);
    let raw_mails = imap_client::fetch_mails_since_uid(&mut session, "INBOX", max_uid).await?;
    eprintln!("[sync] fetched {} raw mails", raw_mails.len());

    let mut count = 0u32;
    {
        let conn = state.0.lock().map_err(|e| AppError::OAuth(e.to_string()))?;
        for (uid, body) in &raw_mails {
            if let Some(mail) = mime_parser::parse_mime(body, account_id, "INBOX", *uid) {
                mails::insert_mail(&conn, &mail)?;
                count += 1;
            }
        }
    }
    eprintln!("[sync] parsed and saved {} mails", count);

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
