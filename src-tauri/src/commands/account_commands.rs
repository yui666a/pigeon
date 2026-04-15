use tauri::State;

use crate::db::accounts;
use crate::error::AppError;
use crate::models::account::{Account, AuthType, CreateAccountRequest};
use crate::state::{DbState, SecureStoreState};

#[tauri::command]
pub fn create_account(
    state: State<DbState>,
    secure_store: State<SecureStoreState>,
    request: CreateAccountRequest,
) -> Result<Account, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    let account = accounts::insert_account(&conn, &request)?;

    // For PLAIN auth, save password to SecureStore
    if matches!(request.auth_type, AuthType::Plain) {
        if let Some(ref password) = request.password {
            crate::commands::auth_commands::save_password(&secure_store.0, &account.id, password)?;
        }
    }

    Ok(account)
}

#[tauri::command]
pub fn get_accounts(state: State<DbState>) -> Result<Vec<Account>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    Ok(accounts::list_accounts(&conn)?)
}

#[tauri::command]
pub fn remove_account(state: State<DbState>, id: String) -> Result<(), AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    Ok(accounts::delete_account(&conn, &id)?)
}
