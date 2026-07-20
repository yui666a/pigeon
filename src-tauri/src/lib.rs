pub mod classifier;
pub mod cli;
pub mod commands;
pub mod context;
pub mod db;
pub mod embedding;
pub mod env_config;
pub mod error;
pub mod mail_chunker;
pub mod mail_sync;
pub mod models;
pub mod project_context;
pub mod project_note_digest;
pub mod project_notes_sync;
pub mod search_normalize;
pub mod search_snippet;
pub mod secure_store;
pub mod state;
pub mod threading;
pub mod usecase;

#[cfg(test)]
pub mod test_helpers;

use db::migrations;
use mail_sync::oauth::OAuthStateStore;
use rusqlite::Connection;
use state::DbState;
use state::EmbeddingRunGuard;
use state::IdleWatchers;
use state::SecureStoreState;
use state::SyncLocks;
use std::sync::Mutex;

/// Tauri のイベントとして進捗を発行する ProgressSink。GUI driver 用。
pub struct TauriProgressSink {
    app: tauri::AppHandle,
}

impl TauriProgressSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl usecase::ProgressSink for TauriProgressSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        use tauri::Emitter;
        // 進捗はベストエフォート（emit 失敗で本処理は止めない）
        let _ = self.app.emit(event, payload);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // cwd 起点で見つからなければ実行ファイル位置から上方探索する。
    // `open` 起動の .app（cwd=/）でも開発時の .env を拾えるようにする（env_config）。
    env_config::load_dotenv();

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Pigeon");

    std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");

    let db_path = data_dir.join("pigeon.db");
    db::vec_ext::register();
    let conn = Connection::open(&db_path).expect("Failed to open database");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("Failed to enable foreign keys");
    migrations::run_migrations(&conn).expect("Failed to run migrations");

    // SecureStore のマスター鍵はデバイス固有の乱数を OS キーチェーンに保管する
    // （ADR 0003）。旧固定鍵のスナップショットは開いた時点で新鍵へ再暗号化する。
    let key_backend = secure_store::default_master_key_backend(&data_dir);
    let key = secure_store::resolve_master_key(key_backend.as_ref())
        .expect("Failed to resolve SecureStore master key");
    let stronghold_path = data_dir.join("pigeon.stronghold");
    let (secure_store, migration) =
        secure_store::SecureStore::open_with_migration(stronghold_path, &key)
            .expect("Failed to initialize SecureStore");
    match &migration {
        secure_store::MasterKeyMigration::MigratedFromLegacy => {
            eprintln!("[info] secure store: 旧固定鍵のスナップショットを新しいマスター鍵で再暗号化しました");
        }
        secure_store::MasterKeyMigration::UnreadableBackedUp { backup } => {
            eprintln!(
                "[warn] secure store: 既存スナップショットを復号できなかったため {} に退避しました。アカウントの再認証が必要です",
                backup.display()
            );
        }
        _ => {}
    }

    // UseCase レジストリ（dispatch バスの能力セット）。全 driver がここを共有する
    let registry = {
        let mut reg = usecase::Registry::new();
        usecase::cases::register_all(&mut reg);
        reg
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(registry)
        .manage(DbState(Mutex::new(conn)))
        .manage(SecureStoreState(secure_store))
        .manage(OAuthStateStore::new())
        .manage(state::ApprovedAttachments::new())
        .manage(SyncLocks::new())
        .manage(IdleWatchers::new())
        .manage(classifier::service::PendingClassifications::new())
        .manage(classifier::service::ClassifyBatches::new())
        .manage(EmbeddingRunGuard::new())
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
                        // バックグラウンドスキャンはベストエフォートだが、行の読み落としを
                        // 黙って空扱いにせず、失敗は警告ログを残してスキップする（B-10）
                        let rows = stmt
                            .query_map([], |row| row.get(0))
                            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<String>>>());
                        match rows {
                            Ok(t) => t,
                            Err(e) => {
                                eprintln!(
                                    "[warn] startup scan: failed to read project_directories: {}",
                                    e
                                );
                                return;
                            }
                        }
                    };
                    if targets.is_empty() {
                        return;
                    }
                    let secure_store = app_handle.state::<SecureStoreState>();
                    let (classifier, cloud) = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let classifier =
                            match classifier::factory::build_classifier(&conn, &secure_store.0) {
                                Ok(c) => c,
                                Err(_) => return,
                            };
                        // クラウドプロバイダ設定時は送信可否ポリシーを適用する
                        // （起動時スキャンも rescan コマンドと同じ境界を通す）
                        let cloud = match classifier::factory::is_cloud_provider_configured(&conn) {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        (classifier, cloud)
                    };
                    for project_id in targets {
                        if let Err(e) = project_context::rescan_project(
                            &db.0,
                            classifier.as_ref(),
                            &project_id,
                            cloud,
                        )
                        .await
                        {
                            eprintln!("[warn] startup scan failed for {}: {}", project_id, e);
                        }
                    }
                });
            }

            // 起動時: 埋め込みキューの消化パスを1回走らせる（v18 埋め込み基盤）。
            // Ollama 停止中でも起動は妨げない（with_conn 内・pass 内のエラーは eprintln! のみ）。
            embedding::worker::spawn_embedding_pass(app.handle(), |_done, _total| {});

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::attachment_commands::list_attachments,
            commands::attachment_commands::save_attachment,
            commands::inline_image_commands::get_inline_images,
            commands::remote_image_commands::fetch_external_images,
            commands::account_commands::create_account,
            commands::account_commands::get_accounts,
            commands::account_commands::remove_account,
            commands::auth_commands::start_oauth,
            commands::auth_commands::handle_oauth_callback,
            commands::mail_commands::sync_account,
            commands::mail_commands::backfill_account,
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
            commands::project_commands::set_project_parent,
            commands::project_commands::archive_project,
            commands::project_commands::delete_project,
            commands::project_commands::merge_projects,
            commands::project_commands::get_effective_context,
            commands::project_commands::get_project_delete_impact,
            commands::saved_search_commands::list_saved_searches,
            commands::saved_search_commands::create_saved_search,
            commands::saved_search_commands::rename_saved_search,
            commands::saved_search_commands::delete_saved_search,
            commands::classify_commands::classify_mail,
            commands::classify_commands::classify_batch,
            commands::classify_commands::cancel_classification,
            commands::classify_commands::approve_classification,
            commands::classify_commands::approve_new_project,
            commands::classify_commands::suggest_project_from_mails,
            commands::classify_commands::reject_classification,
            commands::classify_commands::get_unclassified_threads,
            commands::search_commands::search_mails,
            commands::search_commands::semantic_search,
            commands::send_commands::send_mail,
            commands::send_commands::pick_attachment_files,
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
            commands::project_note_commands::get_project_note,
            commands::project_note_commands::save_project_note_user,
            commands::project_note_commands::save_project_note_ai,
            commands::project_note_commands::generate_project_note_ai,
            commands::project_note_commands::list_project_note_ai_history,
            commands::project_note_commands::restore_project_note_ai,
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
