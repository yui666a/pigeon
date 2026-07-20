use tauri::{AppHandle, State};

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::idle;
use crate::models::account::{Account, AuthType, CreateAccountRequest};
use crate::state::{DbState, SecureStoreState, SyncLocks};
use crate::usecase::{dispatch, Registry};

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

/// アカウント一覧を返す。dispatch バス経由（ADR 0004 D1: 特権的な裏口を作らない）。
#[tauri::command]
pub async fn get_accounts(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
) -> Result<Vec<Account>, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(&registry, "get_accounts", serde_json::json!({}), &ctx).await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected get_accounts output: {e}")))
}

#[tauri::command]
pub fn remove_account(app: AppHandle, state: State<DbState>, id: String) -> Result<(), AppError> {
    // 削除するアカウントの IDLE 監視を停止する
    idle::stop_watching(&app, &id);
    state.with_conn(|conn| accounts::delete_account(conn, &id))
}
