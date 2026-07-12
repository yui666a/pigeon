pub mod classifier;
pub mod commands;
pub mod db;
pub mod error;
pub mod mail_sync;
pub mod models;
pub mod project_context;
pub mod secure_store;
pub mod state;

#[cfg(test)]
pub mod test_helpers;

use db::migrations;
use mail_sync::oauth::OAuthStateStore;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use state::DbState;
use state::IdleWatchers;
use state::SecureStoreState;
use state::SyncLocks;
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(DbState(Mutex::new(conn)))
        .manage(SecureStoreState(secure_store))
        .manage(OAuthStateStore::new())
        .manage(SyncLocks::new())
        .manage(IdleWatchers::new())
        .manage(commands::classify_commands::PendingClassifications::new())
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

            // 起動時: 全アカウントの IMAP IDLE 監視を開始
            // （スペック 2026-07-12-imap-idle-design.md）
            {
                use tauri::Manager;
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let account_ids: Vec<String> = {
                        let db = app_handle.state::<DbState>();
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        match db::accounts::list_accounts(&conn) {
                            Ok(accounts) => accounts.into_iter().map(|a| a.id).collect(),
                            Err(e) => {
                                eprintln!("[warn] idle: failed to list accounts: {}", e);
                                return;
                            }
                        }
                    };
                    for account_id in account_ids {
                        mail_sync::idle::start_watching(&app_handle, &account_id);
                    }
                });
            }

            // 起動時バックグラウンドスキャン（スペック§4）
            {
                use tauri::Manager;
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let db = app_handle.state::<DbState>();
                    let targets: Vec<String> = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let mut stmt =
                            match conn.prepare("SELECT project_id FROM project_directories") {
                                Ok(s) => s,
                                Err(_) => return,
                            };
                        stmt.query_map([], |row| row.get(0))
                            .map(|rows| rows.filter_map(|r| r.ok()).collect())
                            .unwrap_or_default()
                    };
                    if targets.is_empty() {
                        return;
                    }
                    let secure_store = app_handle.state::<SecureStoreState>();
                    let classifier = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        match classifier::factory::build_classifier(&conn, &secure_store.0) {
                            Ok(c) => c,
                            Err(_) => return,
                        }
                    };
                    for project_id in targets {
                        if let Err(e) = project_context::rescan_project(
                            &db.0,
                            classifier.as_ref(),
                            &project_id,
                            false,
                        )
                        .await
                        {
                            eprintln!("[warn] startup scan failed for {}: {}", project_id, e);
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::attachment_commands::list_attachments,
            commands::attachment_commands::save_attachment,
            commands::inline_image_commands::get_inline_images,
            commands::account_commands::create_account,
            commands::account_commands::get_accounts,
            commands::account_commands::remove_account,
            commands::auth_commands::start_oauth,
            commands::auth_commands::handle_oauth_callback,
            commands::mail_commands::sync_account,
            commands::mail_commands::get_threads,
            commands::mail_commands::get_threads_by_project,
            commands::mail_commands::mark_read,
            commands::flag_commands::set_flagged,
            commands::flag_commands::mark_unread,
            commands::mail_commands::get_unread_counts,
            commands::mail_commands::get_recent_unread_subjects,
            commands::project_commands::create_project,
            commands::project_commands::get_projects,
            commands::project_commands::update_project,
            commands::project_commands::archive_project,
            commands::project_commands::delete_project,
            commands::project_commands::merge_projects,
            commands::classify_commands::classify_mail,
            commands::classify_commands::approve_classification,
            commands::classify_commands::approve_new_project,
            commands::classify_commands::reject_classification,
            commands::classify_commands::move_mail,
            commands::classify_commands::get_unclassified_mails,
            commands::classify_commands::get_unclassified_threads,
            commands::classify_commands::get_mails_by_project,
            commands::search_commands::search_mails,
            commands::send_commands::send_mail,
            commands::draft_commands::save_draft,
            commands::draft_commands::get_drafts,
            commands::draft_commands::delete_draft,
            commands::directory_commands::link_project_directory,
            commands::directory_commands::unlink_project_directory,
            commands::directory_commands::get_project_directory,
            commands::directory_commands::rescan_project_directory,
            commands::directory_commands::list_project_files,
            commands::directory_commands::set_cloud_rule,
            commands::directory_commands::get_cloud_rules,
            commands::directory_commands::set_allow_cloud_context,
            commands::directory_commands::get_project_context,
            commands::settings_commands::get_llm_settings,
            commands::settings_commands::set_llm_settings,
            commands::settings_commands::test_llm_connection,
            commands::mail_commands::delete_mail,
            commands::mail_commands::archive_mail,
            commands::mail_commands::unarchive_mail,
            commands::bulk_commands::bulk_delete_mails,
            commands::bulk_commands::bulk_archive_mails,
            commands::bulk_commands::bulk_move_mails,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
