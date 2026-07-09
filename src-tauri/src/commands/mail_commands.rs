use tauri::{AppHandle, Emitter, State};

use crate::db::{accounts, mails};
use crate::error::AppError;
use crate::mail_sync::{imap_client, mime_parser, oauth};
use crate::models::account::{Account, AccountProvider, AuthType};
use crate::models::mail::Thread;
use crate::state::{DbState, SecureStoreState, SyncLocks};

/// sync-progress イベントの payload
#[derive(Clone, serde::Serialize)]
struct SyncProgressEvent {
    account_id: String,
    done: usize,
    total: usize,
}

#[tauri::command]
pub async fn sync_account(
    app: AppHandle,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
) -> Result<u32, AppError> {
    // 同一アカウントの同期が進行中なら開始しない（画面遷移等での多重起動対策）。
    // エラーではなく 0 件を返す: 呼び出し側にとって「新規取り込みなし」と等価
    if !sync_locks.try_begin(&account_id) {
        return Ok(0);
    }
    let result = sync_account_inner(&state, &secure_store.0, &account_id, |done, total| {
        // 進捗はベストエフォート（emit 失敗で同期は止めない）
        let _ = app.emit(
            "sync-progress",
            SyncProgressEvent {
                account_id: account_id.clone(),
                done,
                total,
            },
        );
    })
    .await;
    sync_locks.finish(&account_id);
    result
}

/// Resolve IMAP credentials for the given account.
/// For Google accounts, handles OAuth token refresh if needed.
/// Returns (username, credential) suitable for `imap_client::connect`.
async fn resolve_imap_credentials(
    account: &Account,
    secure_store: &crate::secure_store::SecureStore,
) -> Result<(AuthType, String, String), AppError> {
    match account.provider {
        AccountProvider::Google => {
            let mut token_data =
                match crate::commands::auth_commands::load_oauth_token(secure_store, &account.id) {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!(
                            "[warn] OAuth token not found for account {}: {}",
                            account.id, e
                        );
                        return Err(AppError::ReauthRequired(account.id.clone()));
                    }
                };

            if oauth::token_needs_refresh(&token_data) {
                let config = oauth::OAuthConfig::google()?;
                match oauth::refresh_token(&config, &token_data.refresh_token).await {
                    Ok(response) => {
                        token_data = oauth::build_token_data(
                            &response,
                            &token_data.email,
                            Some(&token_data.refresh_token),
                        )?;
                        crate::commands::auth_commands::save_oauth_token(
                            secure_store,
                            &account.id,
                            &token_data,
                        )?;
                    }
                    Err(e) => {
                        eprintln!(
                            "[warn] Token refresh failed for account {}: {}",
                            account.id, e
                        );
                        return Err(AppError::ReauthRequired(account.id.clone()));
                    }
                }
            }

            Ok((AuthType::Oauth2, token_data.email, token_data.access_token))
        }
        AccountProvider::Other => {
            let password =
                crate::commands::auth_commands::load_password(secure_store, &account.id)?;
            Ok((AuthType::Plain, account.email.clone(), password))
        }
    }
}

async fn sync_account_inner(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<u32, AppError> {
    let (account, max_uid, initial_limit) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let account = accounts::get_account(&conn, account_id)?;
        let max_uid = mails::get_max_uid(&conn, account_id, "INBOX")?;
        let initial_limit =
            crate::db::settings::get_u32_or(&conn, "initial_sync_limit", 5000);
        (account, max_uid, initial_limit)
    };

    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, secure_store).await?;

    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await?;

    let mut count = 0u32;
    let fetch_result = imap_client::fetch_mails_batched(
        &mut session,
        "INBOX",
        max_uid,
        initial_limit,
        |batch, progress| {
            // バッチ単位でロックを取り、挿入してから進捗を通知する
            {
                let conn = state.0.lock().map_err(AppError::lock_err)?;
                for (uid, body) in &batch {
                    if let Some(mail) = mime_parser::parse_mime(body, account_id, "INBOX", *uid) {
                        // 既存行（UNIQUE 重複）は挿入されないため件数に含めない
                        if mails::insert_mail(&conn, &mail)? {
                            count += 1;
                        }
                    }
                }
            }
            on_progress(progress.done, progress.total);
            Ok(())
        },
    )
    .await;

    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    fetch_result?;
    Ok(count)
}

#[tauri::command]
pub fn get_threads(
    state: State<DbState>,
    account_id: String,
    folder: String,
) -> Result<Vec<Thread>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    let all_mails = mails::get_mails_by_account(&conn, &account_id, &folder)?;
    Ok(mails::build_threads(&all_mails))
}

#[tauri::command]
pub fn get_threads_by_project(
    state: State<DbState>,
    project_id: String,
) -> Result<Vec<Thread>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    mails::get_threads_by_project(&conn, &project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{assignments, projects};
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{make_mail, setup_db};

    #[test]
    fn test_get_threads_groups_by_reply() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Hello", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        let all = mails::get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        let threads = mails::build_threads(&all);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_get_threads_empty_account() {
        let conn = setup_db();
        let all = mails::get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        let threads = mails::build_threads(&all);
        assert!(threads.is_empty());
    }

    #[test]
    fn test_get_threads_by_project_builds_threads() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Deal", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Deal", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();
        assignments::assign_mail(&conn, "m2", &proj.id, "ai", Some(0.85)).unwrap();

        let threads = mails::get_threads_by_project(&conn, &proj.id).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_get_threads_by_project_empty() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Empty".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        let threads = mails::get_threads_by_project(&conn, &proj.id).unwrap();
        assert!(threads.is_empty());
    }
}
