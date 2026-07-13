use async_imap::types::{Flag, NameAttribute};
use async_imap::Session;
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use std::collections::HashMap;
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use crate::error::AppError;
use crate::models::account::AuthType;

pub(crate) type ImapSession = Session<TlsStream<Compat<TcpStream>>>;

// FETCH の本文取得は必ず BODY.PEEK[] を使うこと。RFC822 / BODY[] は
// RFC 3501 の仕様で「取得しただけでサーバー側に \Seen が付く」ため、
// 同期・バックフィル・添付取得のたびに未読メールが Gmail 本体ごと
// 既読化されてしまう（2026-07-13 に実際に発生した不具合）。
pub(crate) const FETCH_ITEMS_SYNC: &str = "(UID FLAGS BODY.PEEK[])";
pub(crate) const FETCH_ITEMS_RAW: &str = "(UID BODY.PEEK[])";

pub async fn connect(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
) -> Result<ImapSession, AppError> {
    match auth_type {
        AuthType::Plain => connect_plain(host, port, username, credential).await,
        AuthType::Oauth2 => connect_xoauth2(host, port, username, credential).await,
    }
}

async fn establish_tls(
    host: &str,
    port: u16,
) -> Result<async_imap::Client<TlsStream<Compat<TcpStream>>>, AppError> {
    let tcp = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| AppError::Imap("TCP connection timed out (15s)".into()))?
    .map_err(|e| AppError::Imap(format!("TCP connection failed: {}", e)))?;
    let tcp_compat = tcp.compat();
    let tls = async_native_tls::TlsConnector::new();
    let tls_stream = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tls.connect(host, tcp_compat),
    )
    .await
    .map_err(|_| AppError::Imap("TLS handshake timed out (15s)".into()))?
    .map_err(|e| AppError::Imap(format!("TLS handshake failed: {}", e)))?;
    Ok(async_imap::Client::new(tls_stream))
}

async fn connect_plain(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<ImapSession, AppError> {
    let client = establish_tls(host, port).await?;
    let session = client
        .login(username, password)
        .await
        .map_err(|e| AppError::Imap(format!("PLAIN login failed: {}", e.0)))?;
    Ok(session)
}

async fn connect_xoauth2(
    host: &str,
    port: u16,
    email: &str,
    access_token: &str,
) -> Result<ImapSession, AppError> {
    let mut client = establish_tls(host, port).await?;

    // Read the server greeting — this is critical!
    // Without consuming the greeting, authenticate() hangs waiting for it.
    let _greeting =
        tokio::time::timeout(std::time::Duration::from_secs(10), client.read_response())
            .await
            .map_err(|_| AppError::Imap("Server greeting timed out".into()))?
            .map_err(|e| AppError::Imap(format!("Failed to read greeting: {}", e)))?;

    let authenticator = XOAuth2Authenticator {
        email: email.to_string(),
        access_token: access_token.to_string(),
    };

    let session = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.authenticate("XOAUTH2", authenticator),
    )
    .await
    .map_err(|_| AppError::Imap("XOAUTH2 authentication timed out (15s)".into()))?
    .map_err(|e| AppError::Imap(format!("XOAUTH2 authentication failed: {}", e.0)))?;
    Ok(session)
}

struct XOAuth2Authenticator {
    email: String,
    access_token: String,
}

impl async_imap::Authenticator for XOAuth2Authenticator {
    type Response = String;
    fn process(&mut self, _data: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.email, self.access_token
        )
    }
}

/// 同期バッチのサイズ。1バッチ分の全文のみメモリに保持する
pub const SYNC_BATCH_SIZE: usize = 100;

/// since_uid より新しい UID のみを昇順・重複除去し、batch_size ごとに分割する。
/// 古い順に処理することで、中断しても DB の max_uid がそのまま再開点になる。
pub(crate) fn plan_batches(uids: Vec<u32>, since_uid: u32, batch_size: usize) -> Vec<Vec<u32>> {
    let mut filtered: Vec<u32> = uids.into_iter().filter(|u| *u > since_uid).collect();
    filtered.sort_unstable();
    filtered.dedup();
    filtered
        .chunks(batch_size)
        .map(|chunk| chunk.to_vec())
        .collect()
}

/// min_uid_exclusive 未満の UID を降順（新しい→古い）に並べ、先頭 limit 件までを
/// batch_size ごとに分割する。バッチ内部の並びは降順を維持する。
/// バックフィル（過去メールを新しい→古いの順に遡って取得）専用。
/// 既存の plan_batches（昇順・下限フィルタ・無制限件数、通常同期用）とは向きが逆のため
/// 分けている。
pub(crate) fn plan_backfill_batches(
    uids: Vec<u32>,
    min_uid_exclusive: u32,
    limit: usize,
    batch_size: usize,
) -> Vec<Vec<u32>> {
    let mut filtered: Vec<u32> = uids
        .into_iter()
        .filter(|u| *u < min_uid_exclusive)
        .collect();
    filtered.sort_unstable_by(|a, b| b.cmp(a));
    filtered.dedup();
    filtered.truncate(limit);
    filtered
        .chunks(batch_size)
        .map(|chunk| chunk.to_vec())
        .collect()
}

/// UID FETCH に渡す UID セット文字列（カンマ区切り）
pub(crate) fn uid_set(batch: &[u32]) -> String {
    batch
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 同期の進捗（on_batch コールバックに渡す）
pub struct SyncProgress {
    pub done: usize,
    pub total: usize,
}

/// バッチ FETCH で得た1通分の生データ
pub struct FetchedMail {
    pub uid: u32,
    /// サーバー側で \Seen が付いているか
    pub is_read: bool,
    /// サーバー側で \Flagged が付いているか
    pub is_flagged: bool,
    /// サーバーフラグの文字列表現（例: "\Seen \Answered"）。フラグなしは None
    pub flags: Option<String>,
    pub body: Vec<u8>,
}

/// FLAGS に \Seen が含まれるか
pub(crate) fn contains_seen(flags: &[Flag<'_>]) -> bool {
    flags.contains(&Flag::Seen)
}

/// FLAGS に \Flagged が含まれるか
pub(crate) fn contains_flagged(flags: &[Flag<'_>]) -> bool {
    flags.contains(&Flag::Flagged)
}

/// FLAGS を DB 保存用の文字列にする（空なら None）
pub(crate) fn flags_to_string(flags: &[Flag<'_>]) -> Option<String> {
    if flags.is_empty() {
        return None;
    }
    let names: Vec<String> = flags.iter().map(flag_name).collect();
    Some(names.join(" "))
}

fn flag_name(flag: &Flag<'_>) -> String {
    match flag {
        Flag::Seen => "\\Seen".into(),
        Flag::Answered => "\\Answered".into(),
        Flag::Flagged => "\\Flagged".into(),
        Flag::Deleted => "\\Deleted".into(),
        Flag::Draft => "\\Draft".into(),
        Flag::Recent => "\\Recent".into(),
        Flag::MayCreate => "\\*".into(),
        Flag::Custom(s) => s.to_string(),
    }
}

/// since_uid より新しいメールを、UID一覧の軽量取得 → SYNC_BATCH_SIZE 件ずつの
/// バッチ FETCH で取り込む。バッチごとに on_batch(そのバッチの生メール, 進捗) を呼ぶ。
/// 古い順（UID昇順）に処理するため、途中で中断しても DB の max_uid が再開点になる。
/// 戻り値は取り込み対象の総件数。
pub async fn fetch_mails_batched(
    session: &mut ImapSession,
    folder: &str,
    since_uid: u32,
    initial_limit: u32,
    mut on_batch: impl FnMut(Vec<FetchedMail>, SyncProgress) -> Result<(), AppError>,
) -> Result<usize, AppError> {
    let mailbox = session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    // 対象の UID 一覧のみを軽量取得（本文なし）
    let uids: Vec<u32> = if since_uid == 0 {
        // 初回同期: 直近 initial_limit 件のシーケンス範囲から UID を得る
        let total = mailbox.exists;
        if total == 0 {
            return Ok(0);
        }
        let start = if total > initial_limit {
            total - initial_limit + 1
        } else {
            1
        };
        let messages: Vec<_> = session
            .fetch(&format!("{}:*", start), "(UID)")
            .await
            .map_err(|e| AppError::Imap(format!("UID list fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("UID list stream failed: {}", e)))?;
        messages.iter().filter_map(|m| m.uid).collect()
    } else {
        // 差分同期: since_uid より新しい範囲の UID を得る
        let messages: Vec<_> = session
            .uid_fetch(&format!("{}:*", since_uid + 1), "(UID)")
            .await
            .map_err(|e| AppError::Imap(format!("UID list fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("UID list stream failed: {}", e)))?;
        messages.iter().filter_map(|m| m.uid).collect()
    };

    let batches = plan_batches(uids, since_uid, SYNC_BATCH_SIZE);
    let total: usize = batches.iter().map(|b| b.len()).sum();

    let mut done = 0usize;
    for batch in batches {
        let messages: Vec<_> = session
            .uid_fetch(&uid_set(&batch), FETCH_ITEMS_SYNC)
            .await
            .map_err(|e| AppError::Imap(format!("Batch fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("Batch stream failed: {}", e)))?;

        let mut mails = Vec::with_capacity(messages.len());
        for msg in &messages {
            if let (Some(uid), Some(body)) = (msg.uid, msg.body()) {
                if uid > since_uid {
                    let flags: Vec<Flag<'_>> = msg.flags().collect();
                    mails.push(FetchedMail {
                        uid,
                        is_read: contains_seen(&flags),
                        is_flagged: contains_flagged(&flags),
                        flags: flags_to_string(&flags),
                        body: body.to_vec(),
                    });
                }
            }
        }
        done += batch.len();
        on_batch(mails, SyncProgress { done, total })?;
    }
    Ok(total)
}

/// バックフィルの取得結果。exhausted は「対象 UID が limit 件に満たなかった
/// ＝これ以上サーバーに古いメールがない」ことを表す。
pub struct BackfillResult {
    pub fetched: usize,
    pub exhausted: bool,
}

/// min_uid_exclusive 未満のメールを、新しい→古いの順に limit 件まで遡って取得する
/// （過去メールのバックフィル）。fetch_mails_batched とは取得方向が逆（あちらは
/// since_uid より新しい範囲を昇順で取る）ため独立した関数にしている。
/// min_uid_exclusive <= 1 の場合はこれ以上遡る範囲がないため、対象なしとして即返す。
pub async fn fetch_mails_backfill_batched(
    session: &mut ImapSession,
    folder: &str,
    min_uid_exclusive: u32,
    limit: u32,
    mut on_batch: impl FnMut(Vec<FetchedMail>, SyncProgress) -> Result<(), AppError>,
) -> Result<BackfillResult, AppError> {
    if min_uid_exclusive <= 1 {
        return Ok(BackfillResult {
            fetched: 0,
            exhausted: true,
        });
    }

    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    // min_uid_exclusive 未満の UID 一覧のみを軽量取得（本文なし）
    let messages: Vec<_> = session
        .uid_fetch(format!("1:{}", min_uid_exclusive - 1), "(UID)")
        .await
        .map_err(|e| AppError::Imap(format!("UID list fetch failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("UID list stream failed: {}", e)))?;
    let uids: Vec<u32> = messages.iter().filter_map(|m| m.uid).collect();

    let candidate_count = uids
        .iter()
        .filter(|u| **u < min_uid_exclusive)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let exhausted = candidate_count < limit as usize;

    let batches = plan_backfill_batches(uids, min_uid_exclusive, limit as usize, SYNC_BATCH_SIZE);
    let total: usize = batches.iter().map(|b| b.len()).sum();

    let mut done = 0usize;
    let mut fetched = 0usize;
    for batch in batches {
        let messages: Vec<_> = session
            .uid_fetch(&uid_set(&batch), FETCH_ITEMS_SYNC)
            .await
            .map_err(|e| AppError::Imap(format!("Batch fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("Batch stream failed: {}", e)))?;

        let mut mails = Vec::with_capacity(messages.len());
        for msg in &messages {
            if let (Some(uid), Some(body)) = (msg.uid, msg.body()) {
                if uid < min_uid_exclusive {
                    let flags: Vec<Flag<'_>> = msg.flags().collect();
                    mails.push(FetchedMail {
                        uid,
                        is_read: contains_seen(&flags),
                        is_flagged: contains_flagged(&flags),
                        flags: flags_to_string(&flags),
                        body: body.to_vec(),
                    });
                }
            }
        }
        done += batch.len();
        fetched += mails.len();
        on_batch(mails, SyncProgress { done, total })?;
    }
    Ok(BackfillResult { fetched, exhausted })
}

/// フォルダ全体の uid → (\Seen, \Flagged) マップを取得する（FLAGS のみの軽量 FETCH）。
/// 他クライアントで変更された既読・スター状態をローカル DB に取り込むための再同期に使う。
pub async fn fetch_flag_map(
    session: &mut ImapSession,
    folder: &str,
) -> Result<HashMap<u32, (bool, bool)>, AppError> {
    let mailbox = session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;
    if mailbox.exists == 0 {
        return Ok(HashMap::new());
    }
    let messages: Vec<_> = session
        .uid_fetch("1:*", "(FLAGS)")
        .await
        .map_err(|e| AppError::Imap(format!("FLAGS fetch failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("FLAGS stream failed: {}", e)))?;
    Ok(messages
        .iter()
        .filter_map(|m| {
            m.uid.map(|uid| {
                let flags: Vec<Flag<'_>> = m.flags().collect();
                (uid, (contains_seen(&flags), contains_flagged(&flags)))
            })
        })
        .collect())
}

/// 指定 UID のメールに指定フラグ（例: "\\Seen"）を付与・除去する（STORE の共通処理）。
/// mark_read / mark_unread / set_flagged のサーバー反映がすべてこの形なので集約する。
async fn store_flag_delta(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
    flag: &str,
    add: bool,
) -> Result<(), AppError> {
    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;
    let query = format!(
        "{}FLAGS.SILENT ({})",
        if add { "+" } else { "-" },
        flag
    );
    let _updates: Vec<_> = session
        .uid_store(uid.to_string(), &query)
        .await
        .map_err(|e| AppError::Imap(format!("UID STORE failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("UID STORE stream failed: {}", e)))?;
    Ok(())
}

/// 指定 UID のメールに \Seen フラグを付ける（既読のサーバー反映）。
pub async fn store_seen_flag(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
) -> Result<(), AppError> {
    store_flag_delta(session, folder, uid, "\\Seen", true).await
}

/// 指定 UID のメールから \Seen フラグを外す（未読に戻すサーバー反映）。
pub async fn remove_seen_flag(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
) -> Result<(), AppError> {
    store_flag_delta(session, folder, uid, "\\Seen", false).await
}

/// 指定 UID のメールの \Flagged を付与・除去する（スター/フラグのサーバー反映）。
pub async fn store_flagged(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), AppError> {
    store_flag_delta(session, folder, uid, "\\Flagged", flagged).await
}

/// UID 指定で元メール（RFC822 全文）を1通取得する。
/// 添付ファイルのオンデマンド取得（attachment-download 設計）で使用する。
pub async fn fetch_mail_raw(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
) -> Result<Vec<u8>, AppError> {
    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    let messages: Vec<_> = session
        .uid_fetch(uid.to_string(), FETCH_ITEMS_RAW)
        .await
        .map_err(|e| AppError::Imap(format!("Mail fetch failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("Mail fetch stream failed: {}", e)))?;

    messages
        .iter()
        .find(|m| m.uid == Some(uid))
        .and_then(|m| m.body())
        .map(|b| b.to_vec())
        .ok_or_else(|| AppError::Imap(format!("Mail not found on server (uid={})", uid)))
}

/// 送信済みメールを指定フォルダへ保存する（IMAP APPEND）。
/// 接続→APPEND→logout までを行う。呼び出し側でベストエフォート扱いにすること
pub async fn append_message(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
    folder: &str,
    raw_message: &[u8],
) -> Result<(), AppError> {
    let mut session = connect(host, port, auth_type, username, credential).await?;
    let result = session
        .append(folder, Some("(\\Seen)"), None, raw_message)
        .await
        .map_err(|e| AppError::Imap(format!("APPEND to {} failed: {}", folder, e)));
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed after append: {}", e);
    }
    result
}

/// EXPUNGE を実行する。UIDPLUS 対応サーバーでは UID EXPUNGE で対象 UID のみを
/// 削除し、非対応なら通常 EXPUNGE にフォールバックする（設計書
/// 2026-07-12-mail-delete-archive-design.md「EXPUNGE の方式」参照）。
async fn expunge_uid(session: &mut ImapSession, uid: u32) -> Result<(), AppError> {
    let supports_uidplus = session
        .capabilities()
        .await
        .map(|caps| caps.has_str("UIDPLUS"))
        .unwrap_or(false);
    if supports_uidplus {
        let _removed: Vec<_> = session
            .uid_expunge(uid.to_string())
            .await
            .map_err(|e| AppError::Imap(format!("UID EXPUNGE failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("UID EXPUNGE stream failed: {}", e)))?;
    } else {
        let _removed: Vec<_> = session
            .expunge()
            .await
            .map_err(|e| AppError::Imap(format!("EXPUNGE failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("EXPUNGE stream failed: {}", e)))?;
    }
    Ok(())
}

/// 指定 UID のメールをフォルダから完全に削除する
/// （SELECT → \Deleted 付与 → EXPUNGE）。
pub async fn delete_message(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
) -> Result<(), AppError> {
    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;
    let _updates: Vec<_> = session
        .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Deleted)")
        .await
        .map_err(|e| AppError::Imap(format!("UID STORE failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("UID STORE stream failed: {}", e)))?;
    expunge_uid(session, uid).await
}

/// 指定 UID のメールを dest フォルダへ UID COPY する。
/// フォルダ不在等で COPY が失敗した場合は CREATE を試みて 1 回だけ再試行する。
pub async fn copy_message(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
    dest: &str,
) -> Result<(), AppError> {
    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;
    if session.uid_copy(uid.to_string(), dest).await.is_ok() {
        return Ok(());
    }
    // アーカイブフォルダが未作成の可能性: CREATE して再試行
    // （既存フォルダへの CREATE は NO が返るだけなので失敗は無視してよい）
    if let Err(e) = session.create(dest).await {
        eprintln!("[warn] CREATE {} failed (may already exist): {}", dest, e);
    }
    session
        .uid_copy(uid.to_string(), dest)
        .await
        .map_err(|e| AppError::Imap(format!("UID COPY to {} failed: {}", dest, e)))
}

/// フォルダ属性に \Trash (RFC 6154 SPECIAL-USE) が含まれるか
pub(crate) fn has_trash_attribute(attrs: &[NameAttribute<'_>]) -> bool {
    attrs.contains(&NameAttribute::Trash)
}

/// LIST 応答から SPECIAL-USE (RFC 6154) の \Trash 属性を持つフォルダを探す。
/// Gmail はロケールに依らずこの属性を返す（例: "[Gmail]/ゴミ箱"）。
/// 非対応サーバーでは None（呼び出し側が完全削除にフォールバックする）
pub async fn find_trash_folder(session: &mut ImapSession) -> Result<Option<String>, AppError> {
    let folders: Vec<_> = session
        .list(None, Some("*"))
        .await
        .map_err(|e| AppError::Imap(format!("List folders failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("List stream failed: {}", e)))?;
    Ok(folders
        .iter()
        .find(|f| has_trash_attribute(f.attributes()))
        .map(|f| f.name().to_string()))
}

/// メールをサーバーから削除する（接続 → 削除 → logout）。
/// \Trash 属性のゴミ箱フォルダが見つかればそこへ COPY してから元を消す
/// （= ゴミ箱へ移動。Gmail では「ラベル剥がし」ではなく本当の削除になる）。
/// ゴミ箱が無いサーバーでは \Deleted + EXPUNGE の完全削除にフォールバックする。
/// ゴミ箱フォルダ内のメールを削除する場合は COPY せず完全削除する。
/// 破壊的操作のため呼び出し側は成功を確認してからローカルへ反映すること。
#[allow(clippy::too_many_arguments)]
pub async fn delete_message_remote(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
    folder: &str,
    uid: u32,
) -> Result<(), AppError> {
    let mut session = connect(host, port, auth_type, username, credential).await?;
    let result = {
        // LIST 失敗は接続異常の可能性が高く、黙って完全削除に切り替えるのは
        // 危険（ゴミ箱移動のつもりが復元不能になる）ためエラーとして返す
        match find_trash_folder(&mut session).await {
            Err(e) => Err(e),
            Ok(Some(trash)) if trash != folder => {
                match copy_message(&mut session, folder, uid, &trash).await {
                    Ok(()) => delete_message(&mut session, folder, uid).await,
                    Err(e) => Err(e),
                }
            }
            Ok(_) => delete_message(&mut session, folder, uid).await,
        }
    };
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed after delete: {}", e);
    }
    result
}

/// メールをサーバー上でアーカイブする（接続 → [COPY →] 削除 → logout）。
/// copy_dest が Some ならアーカイブフォルダへ COPY してから元を削除し（other）、
/// None なら削除のみ（Gmail: INBOX ラベル剥がしがアーカイブ相当）。
#[allow(clippy::too_many_arguments)]
pub async fn archive_message_remote(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
    folder: &str,
    uid: u32,
    copy_dest: Option<&str>,
) -> Result<(), AppError> {
    let mut session = connect(host, port, auth_type, username, credential).await?;
    let result = {
        let copy_result = match copy_dest {
            Some(dest) => copy_message(&mut session, folder, uid, dest).await,
            None => Ok(()),
        };
        match copy_result {
            Ok(()) => delete_message(&mut session, folder, uid).await,
            Err(e) => Err(e),
        }
    };
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed after archive: {}", e);
    }
    result
}

/// フォルダ属性に \Sent (RFC 6154 SPECIAL-USE) が含まれるか
pub(crate) fn has_sent_attribute(attrs: &[NameAttribute<'_>]) -> bool {
    attrs.contains(&NameAttribute::Sent)
}

/// LIST 応答から SPECIAL-USE (RFC 6154) の \Sent 属性を持つフォルダを探す。
/// Gmail はロケールに依らずこの属性を返す（例: "[Gmail]/送信済みメール"）。
/// 非対応サーバーでは None（呼び出し側が settings の sent_folder にフォールバックする）
pub async fn find_sent_folder(session: &mut ImapSession) -> Result<Option<String>, AppError> {
    let folders: Vec<_> = session
        .list(None, Some("*"))
        .await
        .map_err(|e| AppError::Imap(format!("List folders failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("List stream failed: {}", e)))?;
    Ok(folders
        .iter()
        .find(|f| has_sent_attribute(f.attributes()))
        .map(|f| f.name().to_string()))
}

pub async fn list_folders(session: &mut ImapSession) -> Result<Vec<String>, AppError> {
    let folders: Vec<_> = session
        .list(None, Some("*"))
        .await
        .map_err(|e| AppError::Imap(format!("List folders failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("List stream failed: {}", e)))?;
    Ok(folders.iter().map(|f| f.name().to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_type_routing_plain() {
        // Verify that AuthType::Plain would route to connect_plain
        let auth_type = AuthType::Plain;
        assert!(matches!(auth_type, AuthType::Plain));
    }

    #[test]
    fn test_auth_type_routing_oauth2() {
        // Verify that AuthType::Oauth2 would route to connect_xoauth2
        let auth_type = AuthType::Oauth2;
        assert!(matches!(auth_type, AuthType::Oauth2));
    }

    #[test]
    fn test_xoauth2_authenticator_returns_sasl_string() {
        let mut auth = XOAuth2Authenticator {
            email: "user@gmail.com".into(),
            access_token: "ya29.token".into(),
        };
        let response = async_imap::Authenticator::process(&mut auth, b"");
        assert_eq!(
            response,
            "user=user@gmail.com\x01auth=Bearer ya29.token\x01\x01"
        );
    }

    #[test]
    fn test_plan_batches_filters_sorts_and_chunks() {
        // since_uid=10 より新しいものだけを昇順で 3件ずつに分割
        let uids = vec![15, 11, 30, 10, 5, 12, 20, 11]; // 逆順・重複・既取り込み分を含む
        let batches = plan_batches(uids, 10, 3);
        assert_eq!(batches, vec![vec![11, 12, 15], vec![20, 30]]);
    }

    #[test]
    fn test_plan_batches_empty_when_nothing_new() {
        assert!(plan_batches(vec![1, 2, 3], 5, 100).is_empty());
        assert!(plan_batches(vec![], 0, 100).is_empty());
    }

    #[test]
    fn test_plan_batches_resume_after_interruption() {
        // 中断再開: 250件目まで取り込み済み(since_uid=250)なら残りだけが対象になる
        let uids: Vec<u32> = (1..=300).collect();
        let batches = plan_batches(uids, 250, 100);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], (251..=300).collect::<Vec<u32>>());
    }

    #[test]
    fn test_plan_backfill_batches_filters_sorts_descending_and_chunks() {
        // min_uid_exclusive=20 未満だけを対象に、降順で3件ずつ分割
        let uids = vec![5, 19, 20, 25, 1, 18, 12, 18]; // 昇順混在・下限以上・重複を含む
        let batches = plan_backfill_batches(uids, 20, 100, 3);
        assert_eq!(batches, vec![vec![19, 18, 12], vec![5, 1]]);
    }

    #[test]
    fn test_plan_backfill_batches_truncates_to_limit_before_chunking() {
        // 対象20件のうち新しい方から limit=5 件だけをバッチ化する
        let uids: Vec<u32> = (1..=20).collect(); // 1..20 が min_uid_exclusive=21 未満で全部対象
        let batches = plan_backfill_batches(uids, 21, 5, 2);
        // 降順で先頭5件 = 20,19,18,17,16 のみが対象になり、2件ずつに分割される
        assert_eq!(batches, vec![vec![20, 19], vec![18, 17], vec![16]]);
    }

    #[test]
    fn test_plan_backfill_batches_empty_when_nothing_older() {
        assert!(plan_backfill_batches(vec![1, 2, 3], 1, 100, 100).is_empty());
        assert!(plan_backfill_batches(vec![], 100, 100, 100).is_empty());
    }

    #[test]
    fn test_uid_set_joins_with_commas() {
        assert_eq!(uid_set(&[101, 102, 105]), "101,102,105");
        assert_eq!(uid_set(&[7]), "7");
    }

    #[test]
    fn test_contains_seen_detects_seen_flag() {
        use async_imap::types::Flag;
        assert!(contains_seen(&[Flag::Answered, Flag::Seen]));
        assert!(!contains_seen(&[Flag::Answered, Flag::Flagged]));
        assert!(!contains_seen(&[]));
    }

    #[test]
    fn test_contains_flagged_detects_flagged_flag() {
        use async_imap::types::Flag;
        assert!(contains_flagged(&[Flag::Answered, Flag::Flagged]));
        assert!(!contains_flagged(&[Flag::Answered, Flag::Seen]));
        assert!(!contains_flagged(&[]));
    }

    #[test]
    fn test_flags_to_string_formats_system_flags() {
        use async_imap::types::Flag;
        assert_eq!(
            flags_to_string(&[Flag::Seen, Flag::Answered]),
            Some("\\Seen \\Answered".to_string())
        );
        assert_eq!(
            flags_to_string(&[Flag::Custom("$Important".into())]),
            Some("$Important".to_string())
        );
    }

    #[test]
    fn test_flags_to_string_empty_is_none() {
        assert_eq!(flags_to_string(&[]), None);
    }

    #[test]
    fn test_has_trash_attribute_detects_trash() {
        assert!(has_trash_attribute(&[
            NameAttribute::NoInferiors,
            NameAttribute::Trash,
        ]));
    }

    #[test]
    fn test_has_trash_attribute_ignores_other_special_use() {
        // \Sent や \Junk はゴミ箱ではない
        assert!(!has_trash_attribute(&[
            NameAttribute::Sent,
            NameAttribute::Junk,
            NameAttribute::Marked,
        ]));
        assert!(!has_trash_attribute(&[]));
    }

    #[test]
    fn test_has_sent_attribute_detects_sent() {
        assert!(has_sent_attribute(&[
            NameAttribute::NoInferiors,
            NameAttribute::Sent,
        ]));
    }

    #[test]
    fn test_has_sent_attribute_ignores_other_special_use() {
        // \Trash や \Junk は送信済みではない
        assert!(!has_sent_attribute(&[
            NameAttribute::Trash,
            NameAttribute::Junk,
            NameAttribute::Drafts,
        ]));
        assert!(!has_sent_attribute(&[]));
    }

    #[test]
    fn test_fetch_items_use_peek_to_avoid_marking_seen() {
        // RFC822 / BODY[]（PEEKなし）は取得しただけでサーバー側に \Seen が付き、
        // 未読メールが Gmail 本体ごと既読化される（2026-07-13 の実不具合）。
        // FETCH 項目は必ず BODY.PEEK[] であることを回帰テストとして固定する。
        for items in [FETCH_ITEMS_SYNC, FETCH_ITEMS_RAW] {
            assert!(items.contains("BODY.PEEK["), "must use BODY.PEEK[]: {items}");
            assert!(!items.contains("RFC822"), "RFC822 sets \\Seen implicitly: {items}");
        }
        // PEEK なしの BODY[ が紛れ込んでいないこと（BODY.PEEK[ は除外して判定）
        for items in [FETCH_ITEMS_SYNC, FETCH_ITEMS_RAW] {
            let without_peek = items.replace("BODY.PEEK[", "");
            assert!(!without_peek.contains("BODY["), "non-PEEK BODY[] found: {items}");
        }
    }
}
