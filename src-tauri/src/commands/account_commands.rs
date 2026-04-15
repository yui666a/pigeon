use tauri::State;

use crate::db::accounts;
use crate::models::account::{Account, AuthType, CreateAccountRequest};
use crate::state::{DbState, SecureStoreState};

#[tauri::command]
pub fn create_account(
    state: State<DbState>,
    secure_store: State<SecureStoreState>,
    request: CreateAccountRequest,
) -> Result<Account, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let account = accounts::insert_account(&conn, &request).map_err(|e| e.to_string())?;

    // For PLAIN auth, save password to SecureStore
    if matches!(request.auth_type, AuthType::Plain) {
        if let Some(ref password) = request.password {
            crate::commands::auth_commands::save_password(&secure_store.0, &account.id, password)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(account)
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
