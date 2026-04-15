pub mod classifier;
pub mod commands;
pub mod db;
pub mod error;
pub mod mail_sync;
pub mod models;
pub mod secure_store;
pub mod state;

#[cfg(test)]
pub mod test_helpers;

use db::migrations;
use mail_sync::oauth::OAuthStateStore;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use state::DbState;
use state::SecureStoreState;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dotenvy::dotenv().ok();

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Pigeon");

    std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");

    let db_path = data_dir.join("pigeon.db");
    let conn = Connection::open(&db_path).expect("Failed to open database");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("Failed to enable foreign keys");
    migrations::run_migrations(&conn).expect("Failed to run migrations");

    // Derive a key for SecureStore from a fixed app-specific salt
    // In production, this would use OS keychain. For now, derive from app identifier.
    let key = Sha256::digest(b"com.haiso666.pigeon-secure-store-key");
    let stronghold_path = data_dir.join("pigeon.stronghold");
    let secure_store = secure_store::SecureStore::new(stronghold_path, &key)
        .expect("Failed to initialize SecureStore");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(DbState(Mutex::new(conn)))
        .manage(SecureStoreState(secure_store))
        .manage(OAuthStateStore::new())
        .manage(commands::classify_commands::PendingClassifications::new())
        .manage(commands::classify_commands::ClassifyCancelFlag::new())
        .setup(|app| {
            // Register deep link handler for OAuth callback
            #[cfg(not(target_os = "android"))]
            {
                use tauri::{Emitter, Listener};
                let handle = app.handle().clone();
                app.listen("deep-link://new-url", move |event| {
                    let urls: Vec<String> =
                        serde_json::from_str(event.payload()).unwrap_or_default();
                    if let Some(url) = urls.first() {
                        if url.starts_with("com.haiso666.pigeon://oauth/callback") {
                            let handle = handle.clone();
                            let url = url.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = handle.emit("oauth-callback", url);
                            });
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::account_commands::create_account,
            commands::account_commands::get_accounts,
            commands::account_commands::remove_account,
            commands::auth_commands::start_oauth,
            commands::auth_commands::handle_oauth_callback,
            commands::mail_commands::sync_account,
            commands::mail_commands::get_threads,
            commands::mail_commands::get_threads_by_project,
            commands::project_commands::create_project,
            commands::project_commands::get_projects,
            commands::project_commands::update_project,
            commands::project_commands::archive_project,
            commands::project_commands::delete_project,
            commands::classify_commands::classify_mail,
            commands::classify_commands::classify_unassigned,
            commands::classify_commands::cancel_classification,
            commands::classify_commands::approve_classification,
            commands::classify_commands::approve_new_project,
            commands::classify_commands::reject_classification,
            commands::classify_commands::move_mail,
            commands::classify_commands::get_unclassified_mails,
            commands::classify_commands::get_mails_by_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
