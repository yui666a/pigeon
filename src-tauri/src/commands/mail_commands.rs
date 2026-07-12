use tauri::{AppHandle, Emitter, State};

use crate::db::{accounts, mails, settings};
use crate::error::AppError;
use crate::mail_sync::{imap_client, mime_parser, oauth};
use crate::models::account::{Account, AccountProvider, AuthType};
use crate::models::mail::{Mail, Thread, UnreadCounts};
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

/// Resolve credentials for the given account (IMAP / SMTP 共用).
/// For Google accounts, handles OAuth token refresh if needed.
/// Returns (auth_type, username, credential) suitable for
/// `imap_client::connect` and `smtp_client::send`.
/// IDLE 監視タスク（mail_sync::idle）も専用接続の認証に再利用する。
pub(crate) async fn resolve_imap_credentials(
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

/// フォルダ取り込み時の DB 反映方式。
/// INBOX は素朴な INSERT OR IGNORE、Sent は message_id マージ（二重行防止・
/// 送信時ローカル行の uid 確定）を使う。
#[derive(Clone, Copy)]
enum MergeStrategy {
    /// UNIQUE(account, folder, uid) で重複を無視して挿入する
    InsertOrIgnore,
    /// message_id で既存行があれば uid を更新、無ければ挿入する（Sent 同期）
    UpsertByMessageId,
}

/// 1フォルダ分を差分取得し、logical_folder でローカル DB に取り込む。
/// server_folder はサーバー上の実フォルダ名（Gmail の Sent 等はロケール依存）、
/// logical_folder はローカル DB 上の正規化名（"INBOX" / "Sent"）。
/// 取り込んだ新規件数を返す。進捗コールバックはバッチごとに呼ぶ。
#[allow(clippy::too_many_arguments)]
async fn sync_folder_into(
    state: &DbState,
    session: &mut imap_client::ImapSession,
    account_id: &str,
    server_folder: &str,
    logical_folder: &str,
    since_uid: u32,
    initial_limit: u32,
    strategy: MergeStrategy,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<u32, AppError> {
    let mut count = 0u32;
    imap_client::fetch_mails_batched(
        session,
        server_folder,
        since_uid,
        initial_limit,
        |batch, progress| {
            // バッチ単位でロックを取り、挿入してから進捗を通知する
            {
                let conn = state.0.lock().map_err(AppError::lock_err)?;
                for fetched in batch {
                    if let Some(mail) = mime_parser::parse_mime(
                        &fetched.body,
                        account_id,
                        logical_folder,
                        fetched.uid,
                        fetched.is_read,
                        fetched.flags,
                    ) {
                        let inserted = match strategy {
                            MergeStrategy::InsertOrIgnore => mails::insert_mail(&conn, &mail)?,
                            MergeStrategy::UpsertByMessageId => {
                                mails::upsert_sent_mail(&conn, &mail)?
                            }
                        };
                        // 既存行の無視・uid 更新のみは新規取り込みに数えない
                        if inserted {
                            count += 1;
                        }
                    }
                }
            }
            on_progress(progress.done, progress.total);
            Ok(())
        },
    )
    .await?;
    Ok(count)
}

/// Sent フォルダをベストエフォートで同期する。
/// サーバー実フォルダは \Sent SPECIAL-USE で探し、無ければ settings の sent_folder。
/// ローカルは logical folder "Sent" に正規化し、message_id マージで取り込む
/// （送信時ローカル行の uid 確定・他クライアント送信の取り込み）。
/// 失敗は警告ログのみ（INBOX 同期の成功を覆さない）。取り込んだ新規件数を返す。
async fn sync_sent_folder(
    state: &DbState,
    session: &mut imap_client::ImapSession,
    account_id: &str,
    initial_limit: u32,
) -> u32 {
    let sent_since = {
        let conn = match state.0.lock() {
            Ok(c) => c,
            Err(_) => return 0,
        };
        // 送信時の推定 uid（uid_confirmed=0）を watermark に含めるとサーバー行が
        // スキップされ reconciliation が成立しないため、確定行のみで計算する（C1）
        mails::get_max_confirmed_uid(&conn, account_id, "Sent").unwrap_or(0)
    };

    let server_folder = match imap_client::find_sent_folder(session).await {
        Ok(Some(name)) => name,
        Ok(None) => {
            let conn = match state.0.lock() {
                Ok(c) => c,
                Err(_) => return 0,
            };
            crate::db::settings::get_or_default(&conn, "sent_folder", "Sent")
        }
        Err(e) => {
            eprintln!("[warn] Sent folder discovery failed: {}", e);
            return 0;
        }
    };

    match sync_folder_into(
        state,
        session,
        account_id,
        &server_folder,
        "Sent",
        sent_since,
        initial_limit,
        MergeStrategy::UpsertByMessageId,
        |_, _| {},
    )
    .await
    {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[warn] Sent folder sync failed: {}", e);
            0
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
        let initial_limit = crate::db::settings::get_u32_or(&conn, "initial_sync_limit", 5000);
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

    let fetch_result = sync_folder_into(
        state,
        &mut session,
        account_id,
        "INBOX",
        "INBOX",
        max_uid,
        initial_limit,
        MergeStrategy::InsertOrIgnore,
        &mut on_progress,
    )
    .await;
    let mut count = *fetch_result.as_ref().unwrap_or(&0);

    // フラグ再同期: 既知メールの既読状態をサーバーに合わせる
    // （他クライアントで既読にした変更の取り込み。設計書「フラグ変更→ローカルDB更新」）。
    // 取り込み自体は成功しているため、ここの失敗は同期エラーにしない
    if fetch_result.is_ok() {
        match imap_client::fetch_seen_map(&mut session, "INBOX").await {
            Ok(seen_map) => {
                let update_result = state
                    .0
                    .lock()
                    .map_err(AppError::lock_err)
                    .and_then(|conn| mails::update_read_flags(&conn, account_id, "INBOX", &seen_map));
                if let Err(e) = update_result {
                    eprintln!("[warn] read-flag DB update failed: {}", e);
                }
            }
            Err(e) => eprintln!("[warn] read-flag resync failed: {}", e),
        }

        // Sent フォルダの同期（ベストエフォート）。送信時ローカル行の uid 確定と
        // 他クライアント送信の取り込み。失敗しても INBOX 同期の成功は覆さない
        count += sync_sent_folder(state, &mut session, account_id, initial_limit).await;
    }

    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    fetch_result?;
    Ok(count)
}

/// メールを既読にする。DB は即時更新し、IMAP への \Seen 反映は
/// バックグラウンドでベストエフォート実行する（失敗してもエラーにしない。
/// サーバー側の状態は次回同期のフラグ再同期で収束する）。
#[tauri::command]
pub fn mark_read(
    app: AppHandle,
    state: State<'_, DbState>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let (folder, uid) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        mails::mark_read(&conn, &mail_id)?
    };

    // サーバー反映は同期処理と独立の都度接続で行い、UI をブロックしない
    tauri::async_runtime::spawn(async move {
        if let Err(e) = push_seen_flag(&app, &account_id, &folder, uid).await {
            eprintln!(
                "[warn] mark_read: failed to set \\Seen on server (mail {}, uid {}): {}",
                mail_id, uid, e
            );
        }
    });
    Ok(())
}

/// IMAP に接続して指定メールへ \Seen フラグを付ける（mark_read のバックグラウンド処理）。
async fn push_seen_flag(
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
    let secure_store = app.state::<SecureStoreState>();
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
    let store_result = imap_client::store_seen_flag(&mut session, folder, uid).await;
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    store_result
}

/// 削除のサーバー反映方式（設計書 2026-07-12-mail-delete-archive-design.md）
#[derive(Debug, PartialEq)]
enum DeletePlan {
    /// サーバーで削除後にローカル行を削除する。
    /// サーバー側は \Trash フォルダがあればゴミ箱へ移動、なければ完全削除
    /// （imap_client::delete_message_remote 参照）
    Server,
    /// ローカル行の削除のみ。Sent フォルダ同期（2026-07-12-sent-sync-uidplus-design.md）
    /// により送信後の Sent 行の uid は後追いで確定するが、同期前の送信直後の行は
    /// 推定 uid のままで、確定済みかをローカル行から判定する手段が現状ない。
    /// 破壊的操作での誤爆を避けるため Sent は安全側で LocalOnly を維持する（v1 制限）。
    LocalOnly,
}

fn plan_delete(folder: &str) -> DeletePlan {
    if folder == "Sent" {
        DeletePlan::LocalOnly
    } else {
        DeletePlan::Server
    }
}

/// アーカイブのサーバー反映方式
#[derive(Debug, PartialEq)]
enum ArchivePlan {
    /// COPY せず \Deleted + EXPUNGE のみ（Gmail: INBOX ラベル剥がし = アーカイブ）
    DeleteOnly,
    /// archive_folder へ UID COPY してから \Deleted + EXPUNGE（一般 IMAP）
    CopyThenDelete(String),
    /// ローカルの folder 更新のみ（Sent。DeletePlan::LocalOnly と同じ理由で
    /// uid 確定状態の判定手段が未整備のため安全側で維持。v1 制限）
    LocalOnly,
}

fn plan_archive(provider: &AccountProvider, folder: &str, archive_folder: &str) -> ArchivePlan {
    if folder == "Sent" {
        return ArchivePlan::LocalOnly;
    }
    match provider {
        AccountProvider::Google => ArchivePlan::DeleteOnly,
        AccountProvider::Other => ArchivePlan::CopyThenDelete(archive_folder.to_string()),
    }
}

/// 削除・アーカイブ対象のアカウントとメールを読み込む。
/// アカウント不在は AccountNotFound、メール不在は MailNotFound。
fn load_mail_context(
    conn: &rusqlite::Connection,
    account_id: &str,
    mail_id: &str,
) -> Result<(Account, Mail), AppError> {
    let account = accounts::get_account(conn, account_id)?;
    let mail = mails::get_mail_by_id(conn, mail_id)?;
    Ok((account, mail))
}

/// メールを削除する。サーバー処理（\Deleted + EXPUNGE）を同期的に実行し、
/// 成功した場合のみローカル DB から行を削除する（既読と違い楽観更新しない。
/// 案件割り当て等は CASCADE で消える）。
#[tauri::command]
pub async fn delete_mail(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let (account, mail) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        load_mail_context(&conn, &account_id, &mail_id)?
    };

    // サーバー処理中は DB ロックを保持しない
    if plan_delete(&mail.folder) == DeletePlan::Server {
        let (auth_type, username, credential) =
            resolve_imap_credentials(&account, &secure_store.0).await?;
        imap_client::delete_message_remote(
            &account.imap_host,
            account.imap_port,
            &auth_type,
            &username,
            &credential,
            &mail.folder,
            mail.uid,
        )
        .await?;
    }

    // サーバー成功後にのみローカルへ反映する（設計書「エラー・順序の原則」）
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    mails::delete_mail(&conn, &mail_id)
}

/// メールをアーカイブする。サーバー処理を同期的に実行し、成功した場合のみ
/// ローカルの folder を 'Archive' に更新する。行は残るため案件割り当て・
/// スレッド・検索は維持される。
#[tauri::command]
pub async fn archive_mail(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let (account, mail, archive_folder) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let (account, mail) = load_mail_context(&conn, &account_id, &mail_id)?;
        let archive_folder = settings::get_or_default(&conn, "archive_folder", "Archive");
        (account, mail, archive_folder)
    };

    let plan = plan_archive(&account.provider, &mail.folder, &archive_folder);
    if plan != ArchivePlan::LocalOnly {
        let copy_dest = match &plan {
            ArchivePlan::CopyThenDelete(dest) => Some(dest.as_str()),
            _ => None,
        };
        let (auth_type, username, credential) =
            resolve_imap_credentials(&account, &secure_store.0).await?;
        imap_client::archive_message_remote(
            &account.imap_host,
            account.imap_port,
            &auth_type,
            &username,
            &credential,
            &mail.folder,
            mail.uid,
            copy_dest,
        )
        .await?;
    }

    let conn = state.0.lock().map_err(AppError::lock_err)?;
    mails::update_folder(&conn, &mail_id, "Archive")
}

/// メールをアーカイブ解除する（folder を 'INBOX' に戻す）。
/// v1 ではローカル更新のみ: アーカイブ時に COPY 先の UID（COPYUID）を保存して
/// おらずサーバー上のメールを特定できないため、サーバー反映は行わない
/// （設計書「アーカイブ解除」参照。サーバー側はアーカイブ済みのまま残る）。
#[tauri::command]
pub fn unarchive_mail(
    state: State<DbState>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    unarchive_mail_inner(&conn, &account_id, &mail_id)
}

fn unarchive_mail_inner(
    conn: &rusqlite::Connection,
    account_id: &str,
    mail_id: &str,
) -> Result<(), AppError> {
    let (_account, mail) = load_mail_context(conn, account_id, mail_id)?;
    if mail.folder != "Archive" {
        return Err(AppError::Validation(format!(
            "アーカイブ済みでないメールは解除できません (folder: {})",
            mail.folder
        )));
    }
    mails::update_folder(conn, mail_id, "INBOX")
}

/// プロジェクト毎 + 未分類の未読件数を返す（INBOX のみ対象）。
#[tauri::command]
pub fn get_unread_counts(
    state: State<DbState>,
    account_id: String,
) -> Result<UnreadCounts, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    mails::get_unread_counts(&conn, &account_id)
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
    fn test_plan_delete_inbox_requires_server() {
        assert_eq!(plan_delete("INBOX"), DeletePlan::Server);
        assert_eq!(plan_delete("Archive"), DeletePlan::Server);
    }

    #[test]
    fn test_plan_delete_sent_is_local_only() {
        // Sent の uid は APPEND 時の推定値でサーバー UID と不一致の可能性がある
        // ため v1 ではサーバー反映しない（設計書「v1 の制限」）
        assert_eq!(plan_delete("Sent"), DeletePlan::LocalOnly);
    }

    #[test]
    fn test_plan_archive_google_deletes_without_copy() {
        // Gmail は INBOX からの削除 = ラベル剥がしがアーカイブ相当
        assert_eq!(
            plan_archive(&AccountProvider::Google, "INBOX", "Archive"),
            ArchivePlan::DeleteOnly
        );
    }

    #[test]
    fn test_plan_archive_other_copies_to_archive_folder() {
        assert_eq!(
            plan_archive(&AccountProvider::Other, "INBOX", "MyArchive"),
            ArchivePlan::CopyThenDelete("MyArchive".to_string())
        );
    }

    #[test]
    fn test_plan_archive_sent_is_local_only() {
        assert_eq!(
            plan_archive(&AccountProvider::Google, "Sent", "Archive"),
            ArchivePlan::LocalOnly
        );
        assert_eq!(
            plan_archive(&AccountProvider::Other, "Sent", "Archive"),
            ArchivePlan::LocalOnly
        );
    }

    #[test]
    fn test_load_mail_context_returns_account_and_mail() {
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-12T10:00:00");
        mails::insert_mail(&conn, &mail).unwrap();

        let (account, loaded) = load_mail_context(&conn, "acc1", "m1").unwrap();
        assert_eq!(account.id, "acc1");
        assert_eq!(loaded.id, "m1");
        assert_eq!(loaded.folder, "INBOX");
    }

    #[test]
    fn test_load_mail_context_missing_account() {
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-12T10:00:00");
        mails::insert_mail(&conn, &mail).unwrap();

        let result = load_mail_context(&conn, "missing", "m1");
        assert!(matches!(result, Err(AppError::AccountNotFound(_))));
    }

    #[test]
    fn test_load_mail_context_missing_mail() {
        let conn = setup_db();
        let result = load_mail_context(&conn, "acc1", "missing");
        assert!(matches!(result, Err(AppError::MailNotFound(_))));
    }

    #[test]
    fn test_unarchive_moves_archived_mail_back_to_inbox() {
        let conn = setup_db();
        let mut mail = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-12T10:00:00");
        mail.folder = "Archive".into();
        mails::insert_mail(&conn, &mail).unwrap();

        unarchive_mail_inner(&conn, "acc1", "m1").unwrap();

        let updated = mails::get_mail_by_id(&conn, "m1").unwrap();
        assert_eq!(updated.folder, "INBOX");
    }

    #[test]
    fn test_unarchive_rejects_non_archived_mail() {
        // INBOX のメールに対する解除は誤操作なので Validation エラー
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-12T10:00:00");
        mails::insert_mail(&conn, &mail).unwrap();

        let result = unarchive_mail_inner(&conn, "acc1", "m1");
        assert!(matches!(result, Err(AppError::Validation(_))));

        // ローカル状態は変更されない
        let unchanged = mails::get_mail_by_id(&conn, "m1").unwrap();
        assert_eq!(unchanged.folder, "INBOX");
    }

    #[test]
    fn test_unarchive_missing_account() {
        let conn = setup_db();
        let mut mail = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-07-12T10:00:00");
        mail.folder = "Archive".into();
        mails::insert_mail(&conn, &mail).unwrap();

        let result = unarchive_mail_inner(&conn, "missing", "m1");
        assert!(matches!(result, Err(AppError::AccountNotFound(_))));
    }

    #[test]
    fn test_unarchive_missing_mail() {
        let conn = setup_db();
        let result = unarchive_mail_inner(&conn, "acc1", "missing");
        assert!(matches!(result, Err(AppError::MailNotFound(_))));
    }

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
