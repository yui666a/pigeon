use tauri::State;

use crate::commands::mail_commands::{push_flag_remote, resolve_imap_credentials, RemoteFlagOp};
use crate::commands::mail_policy::is_local_only_folder;
use crate::db::{accounts, mails};
use crate::error::AppError;
use crate::state::{DbState, SecureStoreState};

/// メールのスター/フラグ（\Flagged）を設定する。
#[tauri::command]
pub async fn set_flagged(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_id: String,
    flagged: bool,
) -> Result<(), AppError> {
    set_flagged_service(&state, &secure_store.0, &account_id, &mail_id, flagged).await
}

/// set_flagged の本体（Ctx 非依存な service 関数。use case と command が共用）。
/// DB は即時更新し、IMAP への \Flagged 反映はバックグラウンドで
/// ベストエフォート実行する（mark_read と同型）。
pub(crate) async fn set_flagged_service(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
    mail_id: &str,
    flagged: bool,
) -> Result<(), AppError> {
    let (folder, uid) = state.with_conn(|conn| mails::set_flagged(conn, mail_id, flagged))?;

    if is_local_only_folder(&folder) {
        return Ok(());
    }

    let account = state.with_conn(|conn| accounts::get_account(conn, account_id))?;
    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, secure_store).await?;

    let mail_id = mail_id.to_string();
    let op = if flagged {
        RemoteFlagOp::SetFlagged
    } else {
        RemoteFlagOp::RemoveFlagged
    };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = push_flag_remote(
            &account,
            &auth_type,
            &username,
            &credential,
            &folder,
            uid,
            op,
        )
        .await
        {
            eprintln!(
                "[warn] set_flagged: failed to set \\Flagged on server (mail {}, uid {}): {}",
                mail_id, uid, e
            );
        }
    });
    Ok(())
}

/// メールを未読に戻す。
#[tauri::command]
pub async fn mark_unread(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    mark_unread_service(&state, &secure_store.0, &account_id, &mail_id).await
}

/// mark_unread の本体（Ctx 非依存な service 関数。use case と command が共用）。
/// DB は即時更新し、IMAP への \Seen 除去はバックグラウンドで
/// ベストエフォート実行する（mark_read の逆）。
pub(crate) async fn mark_unread_service(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
    mail_id: &str,
) -> Result<(), AppError> {
    let (folder, uid) = state.with_conn(|conn| mails::mark_unread(conn, mail_id))?;

    if is_local_only_folder(&folder) {
        return Ok(());
    }

    let account = state.with_conn(|conn| accounts::get_account(conn, account_id))?;
    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, secure_store).await?;

    let mail_id = mail_id.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = push_flag_remote(
            &account,
            &auth_type,
            &username,
            &credential,
            &folder,
            uid,
            RemoteFlagOp::RemoveSeen,
        )
        .await
        {
            eprintln!(
                "[warn] mark_unread: failed to remove \\Seen on server (mail {}, uid {}): {}",
                mail_id, uid, e
            );
        }
    });
    Ok(())
}
