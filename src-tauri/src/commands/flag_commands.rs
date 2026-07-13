use tauri::{AppHandle, State};

use crate::commands::mail_commands::resolve_imap_credentials;
use crate::commands::mail_policy::is_local_only_folder;
use crate::db::{accounts, mails};
use crate::error::AppError;
use crate::mail_sync::imap_client;
use crate::state::DbState;

/// メールのスター/フラグ（\Flagged）を設定する。DB は即時更新し、IMAP への
/// \Flagged 反映はバックグラウンドでベストエフォート実行する（mark_read と同型）。
#[tauri::command]
pub fn set_flagged(
    app: AppHandle,
    state: State<'_, DbState>,
    account_id: String,
    mail_id: String,
    flagged: bool,
) -> Result<(), AppError> {
    let (folder, uid) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        mails::set_flagged(&conn, &mail_id, flagged)?
    };

    if is_local_only_folder(&folder) {
        return Ok(());
    }

    tauri::async_runtime::spawn(async move {
        if let Err(e) = push_flagged(&app, &account_id, &folder, uid, flagged).await {
            eprintln!(
                "[warn] set_flagged: failed to set \\Flagged on server (mail {}, uid {}): {}",
                mail_id, uid, e
            );
        }
    });
    Ok(())
}

/// メールを未読に戻す。DB は即時更新し、IMAP への \Seen 除去はバックグラウンドで
/// ベストエフォート実行する（mark_read の逆）。
#[tauri::command]
pub fn mark_unread(
    app: AppHandle,
    state: State<'_, DbState>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let (folder, uid) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        mails::mark_unread(&conn, &mail_id)?
    };

    if is_local_only_folder(&folder) {
        return Ok(());
    }

    tauri::async_runtime::spawn(async move {
        if let Err(e) = push_unseen_flag(&app, &account_id, &folder, uid).await {
            eprintln!(
                "[warn] mark_unread: failed to remove \\Seen on server (mail {}, uid {}): {}",
                mail_id, uid, e
            );
        }
    });
    Ok(())
}

/// IMAP に接続して指定メールの \Flagged を付与・除去する（set_flagged のバックグラウンド処理）。
async fn push_flagged(
    app: &AppHandle,
    account_id: &str,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), AppError> {
    use tauri::Manager;

    let account = {
        let db = app.state::<DbState>();
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        accounts::get_account(&conn, account_id)?
    };
    let secure_store = app.state::<crate::state::SecureStoreState>();
    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, &secure_store.0).await?;

    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await?;
    let store_result = imap_client::store_flagged(&mut session, folder, uid, flagged).await;
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    store_result
}

/// IMAP に接続して指定メールから \Seen フラグを外す（mark_unread のバックグラウンド処理）。
async fn push_unseen_flag(
    app: &AppHandle,
    account_id: &str,
    folder: &str,
    uid: u32,
) -> Result<(), AppError> {
    use tauri::Manager;

    let account = {
        let db = app.state::<DbState>();
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        accounts::get_account(&conn, account_id)?
    };
    let secure_store = app.state::<crate::state::SecureStoreState>();
    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, &secure_store.0).await?;

    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await?;
    let store_result = imap_client::remove_seen_flag(&mut session, folder, uid).await;
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    store_result
}
