pub mod commands;
pub mod db;
pub mod error;
pub mod mail_sync;
pub mod models;

use commands::account_commands::DbState;
use db::migrations;
use rusqlite::Connection;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db_path = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Pigeon")
        .join("pigeon.db");

    std::fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create data directory");

    let conn = Connection::open(&db_path).expect("Failed to open database");
    migrations::run_migrations(&conn).expect("Failed to run migrations");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(DbState(Mutex::new(conn)))
        .invoke_handler(tauri::generate_handler![
            commands::account_commands::create_account,
            commands::account_commands::get_accounts,
            commands::account_commands::remove_account,
            commands::mail_commands::sync_account,
            commands::mail_commands::get_threads,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
