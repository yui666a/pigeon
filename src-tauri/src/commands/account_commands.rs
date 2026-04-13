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
