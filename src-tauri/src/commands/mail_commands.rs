use tauri::{AppHandle, Emitter, State};

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::usecase::{dispatch, Registry};

use crate::commands::mail_policy::{plan_archive, plan_delete, ArchivePlan, DeletePlan};
use crate::db::{accounts, mails};
use crate::error::AppError;
use crate::mail_sync::sync_service;
use crate::mail_sync::{imap_client, oauth};
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
    let result = sync_account_locked(&app, &state, &secure_store.0, &account_id).await;
    sync_locks.finish(&account_id);
    result
}

/// sync_account のロック取得後の本体。同期のドメインロジックは
/// mail_sync::sync_service に委譲し、ここでは資格情報解決の注入と
/// sync-progress イベントの emit のみを行う。
async fn sync_account_locked(
    app: &AppHandle,
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
) -> Result<u32, AppError> {
    let account = state.with_conn(|conn| accounts::get_account(conn, account_id))?;
    sync_service::sync_account(
        state,
        &account,
        || resolve_imap_credentials(&account, secure_store),
        |done, total| {
            // 進捗はベストエフォート（emit 失敗で同期は止めない）
            let _ = app.emit(
                "sync-progress",
                SyncProgressEvent {
                    account_id: account_id.to_string(),
                    done,
                    total,
                },
            );
        },
    )
    .await
}

/// backfill-progress イベントの payload。sync-progress とは別イベントにしている
/// （同一アカウントで通常同期とバックフィルの進捗が同時に UI へ届くと区別できないため）。
#[derive(Clone, serde::Serialize)]
struct BackfillProgressEvent {
    account_id: String,
    done: usize,
    total: usize,
}

/// backfill_account の戻り値。exhausted が true なら、これ以上サーバーに
/// 遡れる古いメールがないことをフロントに伝える（ボタン無効化の判定に使う）。
#[derive(Clone, serde::Serialize)]
pub struct BackfillOutcome {
    pub fetched: u32,
    pub exhausted: bool,
}

/// ローカル最古メール（INBOX）より古いメールを、新しい→古いの順に最大 limit 件
/// 遡ってサーバーから取得する（バックログ項目8）。SyncLocks は通常同期と共有し、
/// 同一アカウントの同期・バックフィルが同時に走ることを防ぐ。
#[tauri::command]
pub async fn backfill_account(
    app: AppHandle,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    limit: u32,
) -> Result<BackfillOutcome, AppError> {
    if !sync_locks.try_begin(&account_id) {
        return Ok(BackfillOutcome {
            fetched: 0,
            exhausted: false,
        });
    }
    let result = backfill_account_locked(&app, &state, &secure_store.0, &account_id, limit).await;
    sync_locks.finish(&account_id);
    result
}

/// backfill_account のロック取得後の本体。バックフィルのドメインロジックは
/// mail_sync::sync_service に委譲し、ここでは資格情報解決の注入と
/// backfill-progress イベントの emit、BackfillOutcome への変換のみを行う。
async fn backfill_account_locked(
    app: &AppHandle,
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
    limit: u32,
) -> Result<BackfillOutcome, AppError> {
    let account = state.with_conn(|conn| accounts::get_account(conn, account_id))?;
    let result = sync_service::backfill_account(
        state,
        &account,
        || resolve_imap_credentials(&account, secure_store),
        limit,
        |done, total| {
            let _ = app.emit(
                "backfill-progress",
                BackfillProgressEvent {
                    account_id: account_id.to_string(),
                    done,
                    total,
                },
            );
        },
    )
    .await?;
    Ok(BackfillOutcome {
        fetched: result.fetched as u32,
        exhausted: result.exhausted,
    })
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

/// メールを既読にする。DB は即時更新し、IMAP への \Seen 反映は
/// バックグラウンドでベストエフォート実行する（失敗してもエラーにしない。
/// サーバー側の状態は次回同期のフラグ再同期で収束する）。
#[tauri::command]
pub async fn mark_read(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    dispatch(
        &registry,
        "mark_read",
        serde_json::json!({ "account_id": account_id, "mail_id": mail_id }),
        &ctx,
    )
    .await?;
    Ok(())
}

/// mark_read の本体（Ctx 非依存な service 関数。use case と command が共用）。
/// DB 更新と資格情報の解決は同期的に行い、IMAP への \Seen 反映のみ
/// バックグラウンドでベストエフォート実行する。
pub(crate) async fn mark_read_service(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
    mail_id: &str,
) -> Result<(), AppError> {
    let (folder, uid) = state.with_conn(|conn| mails::mark_read(conn, mail_id))?;

    if crate::commands::mail_policy::is_local_only_folder(&folder) {
        return Ok(());
    }

    let account = state.with_conn(|conn| accounts::get_account(conn, account_id))?;
    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, secure_store).await?;

    // サーバー反映は同期処理と独立の都度接続で行い、UI をブロックしない
    let mail_id = mail_id.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = push_flag_remote(
            &account,
            &auth_type,
            &username,
            &credential,
            &folder,
            uid,
            RemoteFlagOp::SetSeen,
        )
        .await
        {
            eprintln!(
                "[warn] mark_read: failed to set \\Seen on server (mail {}, uid {}): {}",
                mail_id, uid, e
            );
        }
    });
    Ok(())
}

/// バックグラウンドで反映するフラグ操作の種別。
#[derive(Clone, Copy)]
pub(crate) enum RemoteFlagOp {
    SetSeen,
    RemoveSeen,
    SetFlagged,
    RemoveFlagged,
}

/// IMAP に接続して指定メールのフラグを更新する（既読・未読・スターの
/// バックグラウンド処理の共通本体。AppHandle 非依存）。
pub(crate) async fn push_flag_remote(
    account: &Account,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
    folder: &str,
    uid: u32,
    op: RemoteFlagOp,
) -> Result<(), AppError> {
    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        auth_type,
        username,
        credential,
    )
    .await?;
    let store_result = match op {
        RemoteFlagOp::SetSeen => imap_client::store_seen_flag(&mut session, folder, uid).await,
        RemoteFlagOp::RemoveSeen => imap_client::remove_seen_flag(&mut session, folder, uid).await,
        RemoteFlagOp::SetFlagged => {
            imap_client::store_flagged(&mut session, folder, uid, true).await
        }
        RemoteFlagOp::RemoveFlagged => {
            imap_client::store_flagged(&mut session, folder, uid, false).await
        }
    };
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    store_result
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

/// IMAP 資格情報 (auth_type, username, credential)。
/// 削除・アーカイブの共通本体（*_inner）では tokio::sync::OnceCell に包んで
/// 「必要になった時点で一度だけ解決」を表現する（単体は遅延解決、
/// bulk は解決済みを事前投入して全メールで使い回す）。
pub(crate) use crate::mail_sync::sync_service::ImapCredentials;

/// delete_mail の本体。単体（delete_mail）と一括（bulk_delete_mails）の
/// 両方から呼ばれる。サーバー処理（\Deleted + EXPUNGE）を同期的に実行し、
/// 成功した場合のみローカル DB から行を削除する（既読と違い楽観更新しない。
/// 案件割り当て等は CASCADE で消える）。
pub(crate) async fn delete_mail_inner(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account: &Account,
    creds: &tokio::sync::OnceCell<ImapCredentials>,
    mail_id: &str,
) -> Result<(), AppError> {
    // サーバー処理中は DB ロックを保持しない
    let mail = state.with_conn(|conn| mails::get_mail_by_id(conn, mail_id))?;

    if plan_delete(&mail.folder) == DeletePlan::Server {
        let (auth_type, username, credential) = creds
            .get_or_try_init(|| resolve_imap_credentials(account, secure_store))
            .await?;
        imap_client::delete_message_remote(
            &account.imap_host,
            account.imap_port,
            auth_type,
            username,
            credential,
            &mail.folder,
            mail.uid,
        )
        .await?;
    }

    // サーバー成功後にのみローカルへ反映する（設計書「エラー・順序の原則」）
    state.with_conn(|conn| mails::delete_mail(conn, mail_id))?;
    // DB削除成功後、添付キャッシュをベストエフォートで掃除する
    // （失敗しても削除自体は成功扱い。孤児化したディスクリークの防止）
    crate::commands::attachment_commands::remove_attachment_cache_for_mail(mail_id);
    Ok(())
}

/// メールを削除する（単体）。dispatch バス経由（プラン依存 Risk + 監査）。
#[tauri::command]
pub async fn delete_mail(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    dispatch(
        &registry,
        "delete_mail",
        serde_json::json!({ "account_id": account_id, "mail_id": mail_id }),
        &ctx,
    )
    .await?;
    Ok(())
}

/// archive_mail の本体。単体（archive_mail）と一括（bulk_archive_mails）の
/// 両方から呼ばれる。サーバー処理を同期的に実行し、成功した場合のみ
/// ローカルの folder を 'Archive' に更新する。行は残るため案件割り当て・
/// スレッド・検索は維持される。
pub(crate) async fn archive_mail_inner(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account: &Account,
    archive_folder: &str,
    creds: &tokio::sync::OnceCell<ImapCredentials>,
    mail_id: &str,
) -> Result<(), AppError> {
    let mail = state.with_conn(|conn| mails::get_mail_by_id(conn, mail_id))?;

    let plan = plan_archive(&account.provider, &mail.folder, archive_folder);
    if plan != ArchivePlan::LocalOnly {
        let copy_dest = match &plan {
            ArchivePlan::CopyThenDelete(dest) => Some(dest.as_str()),
            _ => None,
        };
        let (auth_type, username, credential) = creds
            .get_or_try_init(|| resolve_imap_credentials(account, secure_store))
            .await?;
        imap_client::archive_message_remote(
            &account.imap_host,
            account.imap_port,
            auth_type,
            username,
            credential,
            &mail.folder,
            mail.uid,
            copy_dest,
        )
        .await?;
    }

    state.with_conn(|conn| mails::update_folder(conn, mail_id, "Archive"))
}

/// メールをアーカイブする（単体）。dispatch バス経由（プラン依存 Risk + 監査）。
#[tauri::command]
pub async fn archive_mail(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    dispatch(
        &registry,
        "archive_mail",
        serde_json::json!({ "account_id": account_id, "mail_id": mail_id }),
        &ctx,
    )
    .await?;
    Ok(())
}

/// メールをアーカイブ解除する（folder を 'INBOX' に戻す）。
/// v1 ではローカル更新のみ: アーカイブ時に COPY 先の UID（COPYUID）を保存して
/// おらずサーバー上のメールを特定できないため、サーバー反映は行わない
/// （設計書「アーカイブ解除」参照。サーバー側はアーカイブ済みのまま残る）。
#[tauri::command]
pub async fn unarchive_mail(
    registry: State<'_, Registry>,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    mail_id: String,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&state, &secure_store, &pending, &batches, &sync_locks);
    dispatch(
        &registry,
        "unarchive_mail",
        serde_json::json!({ "account_id": account_id, "mail_id": mail_id }),
        &ctx,
    )
    .await?;
    Ok(())
}

pub(crate) fn unarchive_mail_inner(
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
    state.with_conn(|conn| mails::get_unread_counts(conn, &account_id))
}

/// デスクトップ通知の件名プレビュー用に、直近の未読メール件名を返す
/// （2026-07-12-desktop-notification-design.md「v2: 通知の強化」）。
#[tauri::command]
pub fn get_recent_unread_subjects(
    state: State<DbState>,
    account_id: String,
    limit: u32,
) -> Result<Vec<String>, AppError> {
    state.with_conn(|conn| mails::get_recent_unread_subjects(conn, &account_id, limit))
}

#[tauri::command]
pub fn get_threads(
    state: State<DbState>,
    account_id: String,
    folder: String,
) -> Result<Vec<Thread>, AppError> {
    let all_mails =
        state.with_conn(|conn| mails::get_mails_by_account(conn, &account_id, &folder))?;
    Ok(mails::build_threads(&all_mails))
}

#[tauri::command]
pub fn get_threads_by_project(
    state: State<DbState>,
    project_id: String,
) -> Result<Vec<Thread>, AppError> {
    state.with_conn(|conn| mails::get_threads_by_project(conn, &project_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{assignments, projects};
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{make_mail, setup_db};

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
