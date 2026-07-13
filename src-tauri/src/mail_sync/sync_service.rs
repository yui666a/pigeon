//! 同期オーケストレーション（同期ドメインロジック）。
//!
//! 「INBOX 同期 → フラグ再同期 → Sent 同期」の流れ、フォルダごとの
//! `MergeStrategy`・watermark の使い分け、Sent フォルダ探索フォールバックを担う。
//! Tauri には依存しない: 進捗通知はコールバック、IMAP 資格情報の解決は
//! 呼び出し側（commands 層）がクロージャで注入する。SyncLocks 制御と
//! 進捗イベントの emit は commands/mail_commands.rs 側の責務。

use std::future::Future;

use rusqlite::Connection;

use crate::db::{mails, settings};
use crate::error::AppError;
use crate::mail_sync::{imap_client, mime_parser};
use crate::models::account::{Account, AuthType};
use crate::state::DbState;

/// IMAP 資格情報 (auth_type, username, credential)。
pub type ImapCredentials = (AuthType, String, String);

/// フォルダ取り込み時の DB 反映方式。
/// INBOX は素朴な INSERT OR IGNORE、Sent は message_id マージ（二重行防止・
/// 送信時ローカル行の uid 確定）を使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeStrategy {
    /// UNIQUE(account, folder, uid) で重複を無視して挿入する
    InsertOrIgnore,
    /// message_id で既存行があれば uid を更新、無ければ挿入する（Sent 同期）
    UpsertByMessageId,
}

impl MergeStrategy {
    /// logical folder（ローカル DB 上の正規化名）から反映方式を決める。
    /// Sent のみ送信時ローカル行との照合が必要なため message_id マージ。
    pub fn for_logical_folder(logical_folder: &str) -> Self {
        if logical_folder == "Sent" {
            Self::UpsertByMessageId
        } else {
            Self::InsertOrIgnore
        }
    }
}

/// 差分同期の watermark（この UID より新しいメールを取得する）を読む。
/// Sent は送信時の推定 uid（uid_confirmed=0）を watermark に含めるとサーバー行が
/// スキップされ message_id マージによる uid 後追い確定が成立しないため、
/// 確定行のみで計算する（設計書 2026-07-12-sent-sync-uidplus-design.md「C1」）。
pub fn read_watermark(
    conn: &Connection,
    account_id: &str,
    logical_folder: &str,
) -> Result<u32, AppError> {
    match MergeStrategy::for_logical_folder(logical_folder) {
        MergeStrategy::InsertOrIgnore => mails::get_max_uid(conn, account_id, logical_folder),
        MergeStrategy::UpsertByMessageId => {
            mails::get_max_confirmed_uid(conn, account_id, logical_folder)
        }
    }
}

/// Sent のサーバー実フォルダ名を決める。
/// \Sent SPECIAL-USE で見つかればそれを使い、無ければ設定値
/// （sent_folder、既定 "Sent"）へフォールバックする。
fn pick_sent_server_folder(discovered: Option<String>, configured: String) -> String {
    discovered.unwrap_or(configured)
}

/// アカウント1件を同期する: INBOX の差分取り込み → フラグ再同期 → Sent 同期。
/// `resolve_credentials` は IMAP 資格情報の解決（OAuth リフレッシュ等）を注入する
/// クロージャ（commands 層が `resolve_imap_credentials` を渡す）。
/// `on_progress(done, total)` は INBOX 取り込みのバッチごとに呼ぶ。
/// 戻り値は新規取り込み件数（INBOX + Sent）。
pub async fn sync_account<F, Fut>(
    state: &DbState,
    account: &Account,
    resolve_credentials: F,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<u32, AppError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<ImapCredentials, AppError>>,
{
    let account_id = account.id.as_str();
    let (max_uid, initial_limit) = state.with_conn(|conn| {
        let max_uid = read_watermark(conn, account_id, "INBOX")?;
        let initial_limit = settings::get_u32_or(conn, "initial_sync_limit", 5000)?;
        Ok((max_uid, initial_limit))
    })?;

    let (auth_type, username, credential) = resolve_credentials().await?;

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
        &mut on_progress,
    )
    .await;
    let mut count = *fetch_result.as_ref().unwrap_or(&0);

    // フラグ再同期: 既知メールの既読状態・スター状態をサーバーに合わせる
    // （他クライアントでの変更の取り込み。設計書「フラグ変更→ローカルDB更新」）。
    // 取り込み自体は成功しているため、ここの失敗は同期エラーにしない
    if fetch_result.is_ok() {
        match imap_client::fetch_flag_map(&mut session, "INBOX").await {
            Ok(flag_map) => {
                let update_result = state.with_conn(|conn| {
                    mails::update_flag_state(conn, account_id, "INBOX", &flag_map)
                });
                if let Err(e) = update_result {
                    eprintln!("[warn] flag-state DB update failed: {}", e);
                }
            }
            Err(e) => eprintln!("[warn] flag-state resync failed: {}", e),
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

/// 1フォルダ分を差分取得し、logical_folder でローカル DB に取り込む。
/// server_folder はサーバー上の実フォルダ名（Gmail の Sent 等はロケール依存）、
/// logical_folder はローカル DB 上の正規化名（"INBOX" / "Sent"）で、
/// DB 反映方式（MergeStrategy）もここから決まる。
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
    mut on_progress: impl FnMut(usize, usize),
) -> Result<u32, AppError> {
    let strategy = MergeStrategy::for_logical_folder(logical_folder);
    let mut count = 0u32;
    imap_client::fetch_mails_batched(
        session,
        server_folder,
        since_uid,
        initial_limit,
        |batch, progress| {
            // バッチ単位でロックを取り、挿入してから進捗を通知する
            state.with_conn(|conn| {
                for fetched in batch {
                    if let Some(mail) = mime_parser::parse_mime(
                        &fetched.body,
                        account_id,
                        logical_folder,
                        fetched.uid,
                        fetched.is_read,
                        fetched.is_flagged,
                        fetched.flags,
                    ) {
                        let inserted = match strategy {
                            MergeStrategy::InsertOrIgnore => mails::insert_mail(conn, &mail)?,
                            MergeStrategy::UpsertByMessageId => {
                                mails::upsert_sent_mail(conn, &mail)?
                            }
                        };
                        // 既存行の無視・uid 更新のみは新規取り込みに数えない
                        if inserted {
                            count += 1;
                        }
                    }
                }
                Ok(())
            })?;
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
    // 読み出し失敗を watermark=0 に丸めると全件再取得になるため、
    // この関数のベストエフォート方針に合わせて警告ログを出して同期をスキップする
    let sent_since = match state.with_conn(|conn| read_watermark(conn, account_id, "Sent")) {
        Ok(uid) => uid,
        Err(e) => {
            eprintln!("[warn] Sent watermark read failed: {}", e);
            return 0;
        }
    };

    let discovered = match imap_client::find_sent_folder(session).await {
        Ok(found) => found,
        Err(e) => {
            eprintln!("[warn] Sent folder discovery failed: {}", e);
            return 0;
        }
    };
    let configured =
        match state.with_conn(|conn| settings::get_or_default(conn, "sent_folder", "Sent")) {
            Ok(folder) => folder,
            Err(e) => {
                eprintln!("[warn] sent_folder setting read failed: {}", e);
                return 0;
            }
        };
    let server_folder = pick_sent_server_folder(discovered, configured);

    match sync_folder_into(
        state,
        session,
        account_id,
        &server_folder,
        "Sent",
        sent_since,
        initial_limit,
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

/// ローカル最古メール（INBOX）より古いメールを、新しい→古いの順に最大 limit 件
/// 遡ってサーバーから取得する（バックフィル）。Sent は対象外
/// （v1 制限、設計書 2026-07-13-mail-backfill-design.md）。
/// `resolve_credentials` は遡る対象がある場合のみ呼ばれる（ローカルが空なら接続しない）。
pub async fn backfill_account<F, Fut>(
    state: &DbState,
    account: &Account,
    resolve_credentials: F,
    limit: u32,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<imap_client::BackfillResult, AppError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<ImapCredentials, AppError>>,
{
    let account_id = account.id.as_str();
    let min_uid = state.with_conn(|conn| mails::get_min_uid(conn, account_id, "INBOX"))?;

    // min_uid=0 はローカルにメールが1件もない（通常の初回同期に任せる範囲）。
    // バックフィルの対象がそもそも存在しないため、接続せずに終える
    if min_uid == 0 {
        return Ok(imap_client::BackfillResult {
            fetched: 0,
            exhausted: true,
        });
    }

    let (auth_type, username, credential) = resolve_credentials().await?;

    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await?;

    let result = backfill_folder_into(
        state,
        &mut session,
        account_id,
        min_uid,
        limit,
        &mut on_progress,
    )
    .await;

    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    result
}

/// ローカル最古 UID 未満のメールを、新しい→古いの順に limit 件まで遡って取り込む
/// （バックフィル）。sync_folder_into と異なり取得方向が逆
/// （imap_client::fetch_mails_backfill_batched を使う）。
async fn backfill_folder_into(
    state: &DbState,
    session: &mut imap_client::ImapSession,
    account_id: &str,
    min_uid_exclusive: u32,
    limit: u32,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<imap_client::BackfillResult, AppError> {
    let mut fetched = 0u32;
    let result = imap_client::fetch_mails_backfill_batched(
        session,
        "INBOX",
        min_uid_exclusive,
        limit,
        |batch, progress| {
            state.with_conn(|conn| {
                for m in batch {
                    if let Some(mail) = mime_parser::parse_mime(
                        &m.body,
                        account_id,
                        "INBOX",
                        m.uid,
                        m.is_read,
                        m.is_flagged,
                        m.flags,
                    ) {
                        if mails::insert_mail(conn, &mail)? {
                            fetched += 1;
                        }
                    }
                }
                Ok(())
            })?;
            on_progress(progress.done, progress.total);
            Ok(())
        },
    )
    .await?;
    Ok(imap_client::BackfillResult {
        fetched: fetched as usize,
        exhausted: result.exhausted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_mail, setup_db};

    #[test]
    fn test_merge_strategy_sent_uses_upsert_by_message_id() {
        assert_eq!(
            MergeStrategy::for_logical_folder("Sent"),
            MergeStrategy::UpsertByMessageId
        );
    }

    #[test]
    fn test_merge_strategy_inbox_uses_insert_or_ignore() {
        assert_eq!(
            MergeStrategy::for_logical_folder("INBOX"),
            MergeStrategy::InsertOrIgnore
        );
    }

    #[test]
    fn test_merge_strategy_other_folders_use_insert_or_ignore() {
        // Sent 以外に message_id マージは不要（送信時ローカル行が無いため）
        assert_eq!(
            MergeStrategy::for_logical_folder("Archive"),
            MergeStrategy::InsertOrIgnore
        );
    }

    #[test]
    fn test_read_watermark_inbox_returns_max_uid() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<m1@ex.com>", "A", "2026-07-13T10:00:00");
        m1.uid = 5;
        let mut m2 = make_mail("m2", "<m2@ex.com>", "B", "2026-07-13T11:00:00");
        m2.uid = 9;
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();

        assert_eq!(read_watermark(&conn, "acc1", "INBOX").unwrap(), 9);
    }

    #[test]
    fn test_read_watermark_sent_excludes_unconfirmed_uid() {
        // 送信時ローカル行の推定 uid（uid_confirmed=0）は watermark を汚染しない（C1）
        let conn = setup_db();
        let mut estimated = make_mail("m1", "<m1@ex.com>", "A", "2026-07-13T10:00:00");
        estimated.folder = "Sent".into();
        estimated.uid = 100;
        estimated.uid_confirmed = false;
        let mut confirmed = make_mail("m2", "<m2@ex.com>", "B", "2026-07-13T11:00:00");
        confirmed.folder = "Sent".into();
        confirmed.uid = 40;
        confirmed.uid_confirmed = true;
        crate::db::mails::insert_mail(&conn, &estimated).unwrap();
        crate::db::mails::insert_mail(&conn, &confirmed).unwrap();

        assert_eq!(read_watermark(&conn, "acc1", "Sent").unwrap(), 40);
    }

    #[test]
    fn test_read_watermark_empty_folder_is_zero() {
        let conn = setup_db();
        assert_eq!(read_watermark(&conn, "acc1", "INBOX").unwrap(), 0);
        assert_eq!(read_watermark(&conn, "acc1", "Sent").unwrap(), 0);
    }

    #[test]
    fn test_pick_sent_server_folder_prefers_special_use() {
        assert_eq!(
            pick_sent_server_folder(Some("[Gmail]/送信済みメール".into()), "Sent".into()),
            "[Gmail]/送信済みメール"
        );
    }

    #[test]
    fn test_pick_sent_server_folder_falls_back_to_setting() {
        assert_eq!(pick_sent_server_folder(None, "Sent".into()), "Sent");
    }
}
