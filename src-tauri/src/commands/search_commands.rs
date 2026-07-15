use tauri::State;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::state::{DbState, SecureStoreState, SyncLocks};
use crate::usecase::{dispatch, Registry};

/// UI driver の全文検索。dispatch バス経由（ADR 0004 D1: 特権的な裏口を作らない）。
#[tauri::command]
pub async fn search_mails(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "search_mails",
        serde_json::json!({ "account_id": account_id, "query": query }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected search output: {e}")))
}
