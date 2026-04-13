use tauri::State;

use crate::commands::account_commands::DbState;
use crate::db::mails;
use crate::mail_sync::{imap_client, mime_parser};
use crate::models::mail::Thread;

#[tauri::command]
pub async fn sync_account(
    state: State<'_, DbState>,
    account_id: String,
    imap_host: String,
    imap_port: u16,
    username: String,
    password: String,
) -> Result<u32, String> {
    let max_uid = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        mails::get_max_uid(&conn, &account_id, "INBOX").map_err(|e| e.to_string())?
    };

    let mut session = imap_client::connect(&imap_host, imap_port, &username, &password)
        .await
        .map_err(|e| e.to_string())?;

    let raw_mails = imap_client::fetch_mails_since_uid(&mut session, "INBOX", max_uid)
        .await
        .map_err(|e| e.to_string())?;

    let mut count = 0u32;
    {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        for (uid, body) in &raw_mails {
            if let Some(mail) = mime_parser::parse_mime(body, &account_id, "INBOX", *uid) {
                mails::insert_mail(&conn, &mail).map_err(|e| e.to_string())?;
                count += 1;
            }
        }
    }

    let _ = session.logout().await;
    Ok(count)
}

#[tauri::command]
pub fn get_threads(
    state: State<DbState>,
    account_id: String,
    folder: String,
) -> Result<Vec<Thread>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let all_mails = mails::get_mails_by_account(&conn, &account_id, &folder)
        .map_err(|e| e.to_string())?;
    Ok(mails::build_threads(&all_mails))
}
