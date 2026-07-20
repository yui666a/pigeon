pub mod bootstrap;
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
pub mod mcp;
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

use mail_sync::oauth::OAuthStateStore;
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

    let data_dir = match bootstrap::resolve_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // GUI と CLI が同じ DB / Stronghold を同時に開くとシークレットが無言で
    // 消える（Stronghold は排他ロックを取らず後勝ちで上書きする）。
    // DB を開く前に排他し、_process_lock を run() の間だけ保持する。
    let _process_lock = match cli::lock::ProcessLock::acquire(&data_dir) {
        Ok(lock) => lock,
        Err(_) => {
            eprintln!(
                "error: Pigeon が既に起動しています（または pigeon-cli 実行中）。既存のプロセスを終了してから再実行してください。"
            );
            std::process::exit(1);
        }
    };

    let conn = match bootstrap::open_db(&data_dir) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("error: failed to open database: {e}");
            std::process::exit(1);
        }
    };

    // SecureStore は遅延初期化する（ADR 0006 決定 1）。
    //
    // Stronghold のオープンはスナップショット暗号化時の scrypt が支配的で、
    // 保管件数に依存せず固定で数十秒かかる。加えて macOS ではキーチェーン
    // 読み出しの初回に OS の許可ダイアログが出るため、ユーザー応答まで無限に
    // ブロックしうる。これらを Builder 構築前に行うとウィンドウすら存在しない
    // 状態で待つことになるが、SecureStore は起動時には一切使われない
    // （実際の利用は sync_account の資格情報解決などユーザー操作時）ため、
    // 初回アクセスまで初期化を遅らせる。
    //
    // 遅延させてよいのは ProcessLock と違い「起動できるかどうかを決めない」
    // 処理だからである。ProcessLock は短時間で完了し、失敗したらアプリが
    // 起動できない処理なので上で同期取得したまま残している。
    //
    // なお、この init が走る時点で ProcessLock が保持されていることは
    // スコープで保証される: `_process_lock` はこの run() のローカル変数で、
    // 下の Builder::run() がイベントループ終了までブロックするため、
    // アプリが動いている間ずっと生存する。SecureStoreState へ触れるのは
    // Tauri コマンドとイベントループ上のタスクだけで、いずれもそれより
    // 長生きしない（詳細は SecureStoreState のドキュメントコメント）。
    let data_dir_for_store = data_dir.clone();
    let secure_store_state = SecureStoreState::lazy(move || {
        let (store, migration) = bootstrap::open_secure_store(&data_dir_for_store)?;
        bootstrap::report_master_key_migration(&migration);
        Ok(store)
    });

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
        .manage(secure_store_state)
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

            // 起動時: SecureStore をバックグラウンドで温めておく（ADR 0006 決定 1）。
            //
            // 遅延初期化は起動ブロックを解消するが、コスト自体は初回の秘密情報
            // アクセス時へ移動するだけである。ウィンドウ表示後にここで先回りして
            // 初期化しておくことで、最初の sync_account でも待たされにくくする。
            //
            // 初期化は SecureStoreState 内で直列化されるため、warming 中に
            // ユーザー操作が来ても二重初期化にはならない（後続は同じ実体を共有する）。
            // 失敗してもアプリは動き続ける: 失敗は記憶されないので、次に秘密情報が
            // 必要になった時点で再試行され、そこで明示的なエラーとして表面化する。
            //
            // ProcessLock: この spawn は GUI プロセス内で走り、run() の
            // `_process_lock` が生きている間しか実行されない（Builder::run() が
            // イベントループ終了まで戻らないため）。よって warming が
            // Stronghold を開く時点で必ず排他ロックを保持している。
            {
                use tauri::Manager;
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // ブロッキングな scrypt を非同期ランタイムのワーカー上で走らせない
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = app_handle.state::<SecureStoreState>().get() {
                            eprintln!("[warn] secure store: 事前初期化に失敗しました（次回アクセス時に再試行します）: {e}");
                        }
                    })
                    .await;
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
                    let secure_store_state = app_handle.state::<SecureStoreState>();
                    // SecureStore の解決は DB ロックを取る「前」に済ませる
                    // （ADR 0006 決定 3: DB ロックを保持したまま他のロックを待たない）。
                    // ここが初回アクセスなら、この時点で Stronghold が初期化される
                    let secure_store = match secure_store_state.get() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("[warn] startup scan: secure store unavailable: {}", e);
                            return;
                        }
                    };
                    let (classifier, cloud) = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let classifier =
                            match classifier::factory::build_classifier(&conn, secure_store) {
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
            commands::project_commands::get_projects_with_directories,
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
        // `RunEvent` を扱うため build して run する。
        //
        // `_process_lock` との関係を補足しておく（取得位置を動かさない理由は L71 の
        // コメントを参照）: tao の `EventLoop::run` は `-> !` で最後に
        // `process::exit` を呼ぶため、`run()` は返らず `_process_lock` は
        // **Drop されない**。flock は OS がプロセス終了時に解放する。
        // したがって以下のハンドラは必ずロックを保持したまま走る。
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| match event {
            // 終了要求の時点で flush する（主経路）。
            //
            // `Exit` まで待つとウィンドウが消えた後にメインスレッドで scrypt が
            // 走り、macOS が「終了の遅いアプリ」として強制終了しうる。
            // `ExitRequested` はウィンドウがまだ存在する段階で発火するため、
            // ここで済ませておく方が確実に書き切れる。
            //
            // `api.prevent_exit()` で終了がキャンセルされても不整合は起きない:
            // flush はメモリ上の状態をスナップショットへ書くだけで何も捨てず、
            // 継続後に新しい遅延書き込みが来れば再び未コミットとして記録される。
            tauri::RunEvent::ExitRequested { .. } => flush_secure_store(app_handle),
            // 保険。`ExitRequested` を経ずに `Exit` へ至る経路と、
            // `ExitRequested` での flush が失敗した場合の再試行を担う
            // （コミット失敗時は未コミット状態へ戻すため、ここで拾い直せる）。
            // 既に書き切れていれば未コミットの変更が無く、scrypt は走らない。
            tauri::RunEvent::Exit => flush_secure_store(app_handle),
            _ => {}
        });
}

/// 遅延コミット中の秘密情報をスナップショットへ確定させる（ADR 0006 決定 4）。
///
/// 遅延対象は再取得可能な値だけなので、失っても再認証には至らない。とはいえ
/// 正常終了で捨てる理由は無いので、終了経路で書き切っておく。未コミットの
/// 変更が無ければスナップショット書き出しは走らないため、繰り返し呼んでも
/// 終了が遅くなることはない。
///
/// 異常終了（強制終了・電源断）ではここは走らない。その場合に失われるのは
/// アクセストークンのみで、次回同期時にリフレッシュトークンから再取得される。
fn flush_secure_store(app_handle: &tauri::AppHandle) {
    use tauri::Manager;
    flush_secure_store_state(&app_handle.state::<SecureStoreState>());
}

/// `flush_secure_store` の本体。`AppHandle` はテストで構築できないため、
/// 判断ロジックだけを state に対する関数として切り出している。
fn flush_secure_store_state(state: &SecureStoreState) {
    // 未初期化なら書き込みも遅延分も存在しない。ここで get() すると
    // 終了時に不要な初期化（数十秒）を走らせてしまうので触らない
    if !state.is_initialized() {
        return;
    }
    match state.get() {
        Ok(store) => {
            if let Err(e) = store.flush() {
                eprintln!("[warn] secure store: 終了時のフラッシュに失敗しました: {e}");
            }
        }
        // is_initialized() が true なので実際には起きないが、
        // 終了処理で panic させないため握り潰してログに残す
        Err(e) => {
            eprintln!("[warn] secure store: 終了時の解決に失敗しました: {e}");
        }
    }
}

#[cfg(test)]
mod exit_flush_tests {
    use super::*;
    use crate::secure_store::{SecureStore, ACCESS_TOKEN_CACHE_PREFIX};

    // 終了経路の flush 配線が外れていないことのリグレッションテスト。
    //
    // Deferrable な書き込みは insert 時点ではコミットされないため、終了時に
    // flush が呼ばれないと未コミットのまま失われる。RunEvent ハンドラが将来
    // 削除されてもここで気づけるようにする。実 Stronghold は使わない
    // （スナップショット I/O が 1 回 55 秒）。

    #[test]
    fn test_exit_flush_reaches_initialized_store() {
        // 遅延コミット対象のキーを書いてから終了処理を走らせる
        let store = SecureStore::in_memory();
        store
            .insert(&format!("{ACCESS_TOKEN_CACHE_PREFIX}acc1"), b"at")
            .unwrap();
        let state = SecureStoreState::ready(store);

        flush_secure_store_state(&state);

        let flushed = state
            .get()
            .unwrap()
            .as_in_memory()
            .expect("in-memory store")
            .flush_count();
        assert_eq!(
            flushed, 1,
            "終了時に flush が呼ばれる（Deferrable な書き込みを未コミットで残さない）"
        );
    }

    #[test]
    fn test_exit_flush_does_not_initialize_unused_store() {
        // 一度も秘密情報に触れずに終了した場合、ここで初期化を走らせない。
        // 走らせると終了時に数十秒の scrypt が発生する
        let state = SecureStoreState::lazy(|| {
            panic!("終了時に SecureStore を初期化してはならない");
        });

        flush_secure_store_state(&state);

        assert!(
            !state.is_initialized(),
            "未初期化のまま終了する（初期化を誘発しない）"
        );
    }

    #[test]
    fn test_exit_flush_is_safe_to_call_twice() {
        // ExitRequested と Exit の二重呼び出し。flush 自体が冪等であり、
        // 二度呼んでもエラーにならないこと
        let store = SecureStore::in_memory();
        let state = SecureStoreState::ready(store);

        flush_secure_store_state(&state);
        flush_secure_store_state(&state);

        let flushed = state
            .get()
            .unwrap()
            .as_in_memory()
            .expect("in-memory store")
            .flush_count();
        assert_eq!(flushed, 2, "二重呼び出しでも安全に完了する");
    }
}
