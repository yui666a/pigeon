use tauri::State;

use crate::db::search;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::state::DbState;

#[tauri::command]
pub fn search_mails(
    state: State<DbState>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    search::search_mails(&conn, &account_id, &query, 100)
}
