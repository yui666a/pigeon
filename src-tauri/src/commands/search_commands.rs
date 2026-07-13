use tauri::State;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::db::search;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::state::{DbState, SecureStoreState, SyncLocks};

#[tauri::command]
pub fn search_mails(
    db: State<DbState>,
    secure_store: State<SecureStoreState>,
    pending: State<PendingClassifications>,
    batches: State<ClassifyBatches>,
    sync_locks: State<SyncLocks>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    ctx.with_conn(|conn| search::search_mails(conn, &account_id, &query, 100))
}
