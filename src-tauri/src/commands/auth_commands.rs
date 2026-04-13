use tauri::State;
use uuid::Uuid;

use crate::commands::account_commands::DbState;
use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::oauth::{self, OAuthConfig, OAuthStateStore, PendingOAuth};
use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest, OAuthTokenData};
use crate::secure_store::SecureStore;

pub struct SecureStoreState(pub SecureStore);

#[tauri::command]
pub async fn start_oauth(
    oauth_store: State<'_, OAuthStateStore>,
    provider: String,
) -> Result<String, String> {
    start_oauth_inner(&oauth_store, &provider).map_err(|e| e.to_string())
}

fn start_oauth_inner(oauth_store: &OAuthStateStore, provider: &str) -> Result<String, AppError> {
    match provider {
        "google" => {
            let config = OAuthConfig::google()?;
            let account_id = Uuid::new_v4().to_string();
            let code_verifier = oauth::generate_code_verifier();
            let code_challenge = oauth::generate_code_challenge(&code_verifier);
            let state = oauth::generate_state();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs();

            oauth_store.store(
                state.clone(),
                PendingOAuth {
                    account_id,
                    code_verifier,
                    created_at: now,
                },
            );

            oauth_store.cleanup_expired();

            let auth_url = oauth::build_auth_url(&config, &state, &code_challenge);
            Ok(auth_url)
        }
        _ => Err(AppError::OAuth(format!(
            "Unsupported OAuth provider: {}",
            provider
        ))),
    }
}

#[tauri::command]
pub async fn handle_oauth_callback(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    oauth_store: State<'_, OAuthStateStore>,
    url: String,
) -> Result<String, String> {
    handle_oauth_callback_inner(&state, &secure_store.0, &oauth_store, &url)
        .await
        .map_err(|e| e.to_string())
}

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

    // Check if the pending OAuth entry has expired
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    if now - pending.created_at > 600 {
        return Err(AppError::OAuthTimeout);
    }

    let config = OAuthConfig::google()?;

    // Exchange authorization code for tokens
    let token_response = oauth::exchange_code(&config, &code, &pending.code_verifier).await?;

    // Extract email from ID token
    let email = match &token_response.id_token {
        Some(id_token) => oauth::decode_id_token_email(id_token)?,
        None => return Err(AppError::OAuth("No ID token in response".into())),
    };

    // Build token data
    let token_data = oauth::build_token_data(&token_response, &email, None)?;

    // Check for duplicate email
    {
        let conn = db_state
            .0
            .lock()
            .map_err(|e| AppError::OAuth(e.to_string()))?;
        if let Some(existing) = accounts::account_exists_by_email(&conn, &email)? {
            return Err(AppError::DuplicateAccount(format!(
                "Account with email {} already exists (id: {})",
                email, existing.id
            )));
        }
    }

    // Save tokens to SecureStore
    save_oauth_token(secure_store, &pending.account_id, &token_data)?;

    // Save account to DB
    let account_result = {
        let conn = db_state
            .0
            .lock()
            .map_err(|e| AppError::OAuth(e.to_string()))?;
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
            // Compensating action: remove token from SecureStore if DB insert fails
            let _ = secure_store.delete(&format!("oauth_{}", pending.account_id));
            Err(e)
        }
    }
}

fn save_oauth_token(
    secure_store: &SecureStore,
    account_id: &str,
    token_data: &OAuthTokenData,
) -> Result<(), AppError> {
    let key = format!("oauth_{}", account_id);
    let value = serde_json::to_vec(token_data)
        .map_err(|e| AppError::Stronghold(format!("Failed to serialize token data: {}", e)))?;
    secure_store.insert(&key, &value)
}

pub fn save_oauth_token_public(
    secure_store: &SecureStore,
    account_id: &str,
    token_data: &OAuthTokenData,
) -> Result<(), AppError> {
    save_oauth_token(secure_store, account_id, token_data)
}

pub fn load_oauth_token(
    secure_store: &SecureStore,
    account_id: &str,
) -> Result<OAuthTokenData, AppError> {
    let key = format!("oauth_{}", account_id);
    let value = secure_store.get(&key)?.ok_or_else(|| {
        AppError::Stronghold(format!("No OAuth token found for account {}", account_id))
    })?;
    let token_data: OAuthTokenData = serde_json::from_slice(&value)
        .map_err(|e| AppError::Stronghold(format!("Failed to deserialize token data: {}", e)))?;
    Ok(token_data)
}

pub fn save_password(
    secure_store: &SecureStore,
    account_id: &str,
    password: &str,
) -> Result<(), AppError> {
    let key = format!("password_{}", account_id);
    let value = serde_json::json!({ "password": password }).to_string();
    secure_store.insert(&key, value.as_bytes())
}

pub fn load_password(secure_store: &SecureStore, account_id: &str) -> Result<String, AppError> {
    let key = format!("password_{}", account_id);
    let value = secure_store.get(&key)?.ok_or_else(|| {
        AppError::Stronghold(format!("No password found for account {}", account_id))
    })?;

    #[derive(serde::Deserialize)]
    struct PasswordData {
        password: String,
    }
    let data: PasswordData = serde_json::from_slice(&value)
        .map_err(|e| AppError::Stronghold(format!("Failed to deserialize password: {}", e)))?;
    Ok(data.password)
}
