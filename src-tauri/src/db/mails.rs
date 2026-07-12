use crate::db::assignments;
use crate::error::AppError;
use crate::models::mail::{Mail, Thread, UnreadCounts};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Column list for SELECT queries on the mails table (no table prefix).
/// Must match the field order expected by `row_to_mail`.
pub const MAIL_COLUMNS: &str = "id, account_id, folder, message_id, in_reply_to, \"references\",
     from_addr, to_addr, cc_addr, subject, body_text, body_html,
     date, has_attachments, raw_size, uid, flags, is_read, is_flagged, fetched_at, uid_confirmed";

/// Number of columns in MAIL_COLUMNS / MAIL_COLUMNS_PREFIXED.
/// JOIN クエリで追加カラムを読む際のオフセットとして使う。
pub const MAIL_COLUMN_COUNT: usize = 21;

/// Column list with `m.` table prefix for JOIN queries.
pub const MAIL_COLUMNS_PREFIXED: &str =
    "m.id, m.account_id, m.folder, m.message_id, m.in_reply_to, m.\"references\",
     m.from_addr, m.to_addr, m.cc_addr, m.subject, m.body_text, m.body_html,
     m.date, m.has_attachments, m.raw_size, m.uid, m.flags, m.is_read, m.is_flagged, m.fetched_at, m.uid_confirmed";

/// Map a rusqlite Row to a Mail struct. Column order must match `MAIL_COLUMNS`.
pub fn row_to_mail(row: &rusqlite::Row<'_>) -> rusqlite::Result<Mail> {
    Ok(Mail {
        id: row.get(0)?,
        account_id: row.get(1)?,
        folder: row.get(2)?,
        message_id: row.get(3)?,
        in_reply_to: row.get(4)?,
        references: row.get(5)?,
        from_addr: row.get(6)?,
        to_addr: row.get(7)?,
        cc_addr: row.get(8)?,
        subject: row.get(9)?,
        body_text: row.get(10)?,
        body_html: row.get(11)?,
        date: row.get(12)?,
        has_attachments: row.get(13)?,
        raw_size: row.get(14)?,
        uid: row.get(15)?,
        flags: row.get(16)?,
        is_read: row.get(17)?,
        is_flagged: row.get(18)?,
        fetched_at: row.get(19)?,
        uid_confirmed: row.get(20)?,
    })
}

/// Load a single mail by ID.
pub fn get_mail_by_id(conn: &Connection, mail_id: &str) -> Result<Mail, AppError> {
    conn.query_row(
        &format!("SELECT {} FROM mails WHERE id = ?1", MAIL_COLUMNS),
        params![mail_id],
        row_to_mail,
    )
    .map_err(|_| AppError::MailNotFound(mail_id.to_string()))
}

/// メールを挿入する。同じ (account_id, folder, uid) の行が既にあれば
/// 何もせず false を返す（挿入したら true）。
/// OR REPLACE にすると UNIQUE 衝突時に既存行が削除され、案件割り当てが
/// CASCADE で消えるため、必ず IGNORE で既存行を守ること。
pub fn insert_mail(conn: &Connection, mail: &Mail) -> Result<bool, AppError> {
    let affected = conn.execute(
        "INSERT OR IGNORE INTO mails
         (id, account_id, folder, message_id, in_reply_to, \"references\",
          from_addr, to_addr, cc_addr, subject, body_text, body_html,
          date, has_attachments, raw_size, uid, flags, is_read, is_flagged, fetched_at, uid_confirmed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            mail.id,
            mail.account_id,
            mail.folder,
            mail.message_id,
            mail.in_reply_to,
            mail.references,
            mail.from_addr,
            mail.to_addr,
            mail.cc_addr,
            mail.subject,
            mail.body_text,
            mail.body_html,
            mail.date,
            mail.has_attachments,
            mail.raw_size,
            mail.uid,
            mail.flags,
            mail.is_read,
            mail.is_flagged,
            mail.fetched_at,
            mail.uid_confirmed,
        ],
    )?;
    Ok(affected > 0)
}

pub fn get_mails_by_account(
    conn: &Connection,
    account_id: &str,
    folder: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM mails WHERE account_id = ?1 AND folder = ?2 ORDER BY date DESC",
        MAIL_COLUMNS
    ))?;
    let mails = stmt
        .query_map(params![account_id, folder], row_to_mail)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(mails)
}

pub fn get_max_uid(conn: &Connection, account_id: &str, folder: &str) -> Result<u32, AppError> {
    let uid: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(uid), 0) FROM mails WHERE account_id = ?1 AND folder = ?2",
            params![account_id, folder],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(uid)
}

/// folder 内の最小 uid を返す（行が無ければ 0）。バックフィルの起点
/// （ここ未満の UID をサーバーへ遡って問い合わせる）に使う。
pub fn get_min_uid(conn: &Connection, account_id: &str, folder: &str) -> Result<u32, AppError> {
    let uid: u32 = conn
        .query_row(
            "SELECT COALESCE(MIN(uid), 0) FROM mails WHERE account_id = ?1 AND folder = ?2",
            params![account_id, folder],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(uid)
}

/// uid_confirmed=1 の行のみで folder 内の max uid を返す（差分同期の watermark 用）。
/// Sent フォルダでは送信時ローカル保存分の uid は推定値（uid_confirmed=0）で、
/// サーバー実 uid より大きくなりがち。これを watermark に含めると実 uid が推定 max 以下の
/// サーバー行が fetch からスキップされ、message_id マージによる uid 後追い確定が成立しない。
/// 確定行のみで watermark を計算することでこの汚染を防ぐ
/// （設計書 2026-07-12-sent-sync-uidplus-design.md「C1」）。
pub fn get_max_confirmed_uid(
    conn: &Connection,
    account_id: &str,
    folder: &str,
) -> Result<u32, AppError> {
    let uid: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(uid), 0) FROM mails
             WHERE account_id = ?1 AND folder = ?2 AND uid_confirmed = 1",
            params![account_id, folder],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(uid)
}

/// サーバーから取得した uid → (\Seen, \Flagged) マップで既知メールの
/// is_read / is_flagged を一括更新する。状態が変わる行のみ UPDATE し、
/// 更新した行数を返す（フラグ再同期用）。
pub fn update_flag_state(
    conn: &Connection,
    account_id: &str,
    folder: &str,
    flags_by_uid: &HashMap<u32, (bool, bool)>,
) -> Result<usize, AppError> {
    let tx = conn.unchecked_transaction()?;
    let mut updated = 0usize;
    {
        let mut stmt = tx.prepare(
            "UPDATE mails SET is_read = ?1, is_flagged = ?2
             WHERE account_id = ?3 AND folder = ?4 AND uid = ?5
               AND (is_read != ?1 OR is_flagged != ?2)",
        )?;
        for (uid, (is_seen, is_flagged)) in flags_by_uid {
            updated += stmt.execute(params![is_seen, is_flagged, account_id, folder, uid])?;
        }
    }
    tx.commit()?;
    Ok(updated)
}

/// メールの行を削除する。mail_project_assignments / mail_attachments /
/// correction_log は CASCADE で消え、FTS はトリガーで削除される。
/// 対象が存在しなければ MailNotFound。
pub fn delete_mail(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    let affected = conn.execute("DELETE FROM mails WHERE id = ?1", params![mail_id])?;
    if affected == 0 {
        return Err(AppError::MailNotFound(mail_id.to_string()));
    }
    Ok(())
}

/// メールのフォルダを更新する（アーカイブ等）。行は残るため案件割り当て・
/// スレッド・検索は維持される。対象が存在しなければ MailNotFound。
pub fn update_folder(conn: &Connection, mail_id: &str, folder: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE mails SET folder = ?1 WHERE id = ?2",
        params![folder, mail_id],
    )?;
    if affected == 0 {
        return Err(AppError::MailNotFound(mail_id.to_string()));
    }
    Ok(())
}

/// account_id + folder + message_id に一致するメールの id を返す（無ければ None）。
/// Sent 同期で、送信時ローカル行とサーバー同期行を突き合わせるのに使う。
pub fn get_mail_id_by_message_id(
    conn: &Connection,
    account_id: &str,
    folder: &str,
    message_id: &str,
) -> Result<Option<String>, AppError> {
    let id = conn
        .query_row(
            "SELECT id FROM mails WHERE account_id = ?1 AND folder = ?2 AND message_id = ?3",
            params![account_id, folder, message_id],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(id)
}

/// account_id + folder + uid を占有している行の (id, message_id) を返す（無ければ None）。
/// UNIQUE(account_id, folder, uid) 衝突の相手を特定するのに使う。
fn find_uid_occupant(
    conn: &Connection,
    account_id: &str,
    folder: &str,
    uid: u32,
) -> Result<Option<(String, String)>, AppError> {
    let row = conn
        .query_row(
            "SELECT id, message_id FROM mails
             WHERE account_id = ?1 AND folder = ?2 AND uid = ?3",
            params![account_id, folder, uid],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .ok();
    Ok(row)
}

/// 行を uid=サーバー値・uid_confirmed=1 に確定させる。行は残るため案件割り当ては維持。
/// 対象が存在しなければ MailNotFound。
/// 注意: (account_id, folder, uid) の UNIQUE 衝突は呼び出し側で事前に解消すること。
pub fn confirm_uid(conn: &Connection, mail_id: &str, uid: u32) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE mails SET uid = ?1, uid_confirmed = 1 WHERE id = ?2",
        params![uid, mail_id],
    )?;
    if affected == 0 {
        return Err(AppError::MailNotFound(mail_id.to_string()));
    }
    Ok(())
}

/// Sent 同期用のマージ挿入。サーバー同期行 `mail`（正しい uid・uid_confirmed=true）を
/// ローカルへ取り込む。同一 (account_id, 'Sent', message_id) の送信時ローカル行があれば
/// その行の uid をサーバー値へ確定して二重行を防ぎ、案件割り当てを保持する。
///
/// uid 確定時に別行が同じ (account_id, 'Sent', uid) を占有していると UNIQUE 衝突するため
/// （送信時の推定 uid とサーバー実 uid が同値を取り合うケース。設計書「C2」）、
/// 衝突を検出して解消する:
/// - 占有行が同一 message_id（同一メールの重複行）: 案件割り当てを持つ側を残して他方を削除し統合
/// - 占有行が異なる message_id: この行の取り込みをスキップして警告（バッチは継続させる）
///
/// 戻り値は「新規の取り込みが起きたか」（同期件数の集計用。確定・統合・スキップは false）。
pub fn upsert_sent_mail(conn: &Connection, mail: &Mail) -> Result<bool, AppError> {
    let existing_by_mid =
        get_mail_id_by_message_id(conn, &mail.account_id, &mail.folder, &mail.message_id)?;

    // uid スロットの占有行（自分自身・message_id 一致行は除外して判定する）
    let occupant = find_uid_occupant(conn, &mail.account_id, &mail.folder, mail.uid)?
        .filter(|(occ_id, _)| Some(occ_id) != existing_by_mid.as_ref());

    match existing_by_mid {
        Some(existing_id) => {
            // 送信時ローカル行が既にある → uid をサーバー値へ確定
            if let Some((occ_id, occ_mid)) = occupant {
                if occ_mid == mail.message_id {
                    // 同一メールの重複行が uid スロットを占有 → 統合（割り当て保持側を残す）
                    merge_duplicate_sent_rows(conn, &existing_id, &occ_id, mail.uid)?;
                    return Ok(false);
                }
                // 異なるメールが占有 → 誤操作防止のためスキップ（バッチは継続）
                eprintln!(
                    "[warn] Sent uid {} occupied by different message ({}); skipping confirm for {}",
                    mail.uid, occ_mid, mail.message_id
                );
                return Ok(false);
            }
            confirm_uid(conn, &existing_id, mail.uid)?;
            Ok(false)
        }
        None => {
            // 送信時ローカル行が無い（他クライアント送信）
            if let Some((_occ_id, occ_mid)) = occupant {
                // 未知メールなのに uid スロットが別メールで埋まっている → スキップ（バッチ継続）
                eprintln!(
                    "[warn] Sent uid {} occupied by different message ({}); skipping import of {}",
                    mail.uid, occ_mid, mail.message_id
                );
                return Ok(false);
            }
            insert_mail(conn, mail)
        }
    }
}

/// 同一メール（同一 message_id）の重複 Sent 行を統合する。案件割り当てを持つ側を残し、
/// 他方を削除して、残した行を uid=サーバー値・uid_confirmed=1 に確定する。
/// 両方とも割り当てを持たなければ keep_id を残す。
fn merge_duplicate_sent_rows(
    conn: &Connection,
    keep_candidate: &str,
    other_candidate: &str,
    uid: u32,
) -> Result<(), AppError> {
    let keep_has = has_project_assignment(conn, keep_candidate)?;
    let other_has = has_project_assignment(conn, other_candidate)?;
    // 割り当てを持つ側を優先して残す（CASCADE で割り当てを失わないため）
    let (keep, drop) = if other_has && !keep_has {
        (other_candidate, keep_candidate)
    } else {
        (keep_candidate, other_candidate)
    };
    // 先に重複行を消してから uid を確定（UNIQUE 衝突を避ける順序）
    delete_mail(conn, drop)?;
    confirm_uid(conn, keep, uid)?;
    Ok(())
}

/// 指定メールに案件割り当てが1件以上あるか。
fn has_project_assignment(conn: &Connection, mail_id: &str) -> Result<bool, AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM mail_project_assignments WHERE mail_id = ?1)",
            params![mail_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    Ok(exists)
}

/// メールを既読にし、サーバー反映に必要な (folder, uid) を返す。
pub fn mark_read(conn: &Connection, mail_id: &str) -> Result<(String, u32), AppError> {
    let (folder, uid): (String, u32) = conn
        .query_row(
            "SELECT folder, uid FROM mails WHERE id = ?1",
            params![mail_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| AppError::MailNotFound(mail_id.to_string()))?;
    conn.execute(
        "UPDATE mails SET is_read = 1 WHERE id = ?1",
        params![mail_id],
    )?;
    Ok((folder, uid))
}

/// メールを未読に戻し、サーバー反映に必要な (folder, uid) を返す（mark_read の逆）。
pub fn mark_unread(conn: &Connection, mail_id: &str) -> Result<(String, u32), AppError> {
    let (folder, uid): (String, u32) = conn
        .query_row(
            "SELECT folder, uid FROM mails WHERE id = ?1",
            params![mail_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| AppError::MailNotFound(mail_id.to_string()))?;
    conn.execute(
        "UPDATE mails SET is_read = 0 WHERE id = ?1",
        params![mail_id],
    )?;
    Ok((folder, uid))
}

/// メールのスター/フラグを設定し、サーバー反映に必要な (folder, uid) を返す。
pub fn set_flagged(
    conn: &Connection,
    mail_id: &str,
    flagged: bool,
) -> Result<(String, u32), AppError> {
    let (folder, uid): (String, u32) = conn
        .query_row(
            "SELECT folder, uid FROM mails WHERE id = ?1",
            params![mail_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| AppError::MailNotFound(mail_id.to_string()))?;
    conn.execute(
        "UPDATE mails SET is_flagged = ?1 WHERE id = ?2",
        params![flagged, mail_id],
    )?;
    Ok((folder, uid))
}

/// 直近の未読メール（INBOX）の件名を新しい順に最大 limit 件返す。
/// デスクトップ通知の件名プレビュー用（2026-07-12-desktop-notification-design.md
/// 「v2: 通知の強化」）。
pub fn get_recent_unread_subjects(
    conn: &Connection,
    account_id: &str,
    limit: u32,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT subject FROM mails
         WHERE account_id = ?1 AND folder = 'INBOX' AND is_read = 0
         ORDER BY date DESC LIMIT ?2",
    )?;
    let subjects = stmt
        .query_map(params![account_id, limit], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(subjects)
}

/// プロジェクト毎 + 未分類の未読件数を集計する（INBOX のみ対象）。
pub fn get_unread_counts(conn: &Connection, account_id: &str) -> Result<UnreadCounts, AppError> {
    let mut stmt = conn.prepare(
        "SELECT mpa.project_id, COUNT(*) FROM mails m
         JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
         WHERE m.account_id = ?1 AND m.folder = 'INBOX' AND m.is_read = 0
         GROUP BY mpa.project_id",
    )?;
    let by_project: HashMap<String, u32> = stmt
        .query_map(params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let unclassified: u32 = conn.query_row(
        "SELECT COUNT(*) FROM mails m
         LEFT JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
         WHERE mpa.mail_id IS NULL AND m.account_id = ?1
           AND m.folder = 'INBOX' AND m.is_read = 0",
        params![account_id],
        |row| row.get(0),
    )?;

    Ok(UnreadCounts {
        by_project,
        unclassified,
    })
}

pub fn build_threads(mails: &[Mail]) -> Vec<Thread> {
    let mut by_message_id: HashMap<&str, usize> = HashMap::new();
    for (i, mail) in mails.iter().enumerate() {
        by_message_id.insert(&mail.message_id, i);
    }

    let mut thread_root: Vec<usize> = (0..mails.len()).collect();

    for (i, mail) in mails.iter().enumerate() {
        if let Some(ref reply_to) = mail.in_reply_to {
            if let Some(&parent_idx) = by_message_id.get(reply_to.as_str()) {
                let root_i = find_root(&thread_root, i);
                let root_p = find_root(&thread_root, parent_idx);
                if root_i != root_p {
                    thread_root[root_i] = root_p;
                }
            }
        }
        if let Some(ref refs) = mail.references {
            for ref_id in refs.split_whitespace() {
                if let Some(&ref_idx) = by_message_id.get(ref_id) {
                    let root_i = find_root(&thread_root, i);
                    let root_r = find_root(&thread_root, ref_idx);
                    if root_i != root_r {
                        thread_root[root_i] = root_r;
                    }
                }
            }
        }
    }

    let normalized: Vec<String> = mails
        .iter()
        .map(|m| normalize_subject(&m.subject))
        .collect();
    for i in 0..mails.len() {
        if mails[i].in_reply_to.is_some() || mails[i].references.is_some() {
            continue;
        }
        for j in 0..i {
            if normalized[i] == normalized[j] {
                let root_i = find_root(&thread_root, i);
                let root_j = find_root(&thread_root, j);
                if root_i != root_j {
                    thread_root[root_i] = root_j;
                }
                break;
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..mails.len() {
        let root = find_root(&thread_root, i);
        groups.entry(root).or_default().push(i);
    }

    let mut threads: Vec<Thread> = groups
        .into_values()
        .map(|indices| {
            let mut thread_mails: Vec<Mail> = indices.iter().map(|&i| mails[i].clone()).collect();
            thread_mails.sort_by(|a, b| a.date.cmp(&b.date));
            let from_addrs: Vec<String> = thread_mails
                .iter()
                .map(|m| m.from_addr.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let last_date = thread_mails
                .last()
                .map(|m| m.date.clone())
                .unwrap_or_default();
            let subject = thread_mails
                .first()
                .map(|m| m.subject.clone())
                .unwrap_or_default();
            let thread_id = thread_mails
                .first()
                .map(|m| m.message_id.clone())
                .unwrap_or_default();
            Thread {
                thread_id,
                subject,
                last_date,
                mail_count: thread_mails.len(),
                from_addrs,
                mails: thread_mails,
            }
        })
        .collect();

    threads.sort_by(|a, b| b.last_date.cmp(&a.last_date));
    threads
}

pub fn get_threads_by_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<Thread>, AppError> {
    let mails = assignments::get_mails_by_project(conn, project_id)?;
    Ok(build_threads(&mails))
}

fn find_root(roots: &[usize], mut i: usize) -> usize {
    while roots[i] != i {
        i = roots[i];
    }
    i
}

fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        if lower.starts_with("re:") || lower.starts_with("fw:") {
            s = s[3..].trim_start();
        } else if lower.starts_with("fwd:") {
            s = s[4..].trim_start();
        } else {
            break;
        }
    }
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_mail, setup_db};

    #[test]
    fn test_insert_and_get_mails() {
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@example.com>", "Hello", "2026-04-13T10:00:00");
        insert_mail(&conn, &mail).unwrap();
        let mails = get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        assert_eq!(mails.len(), 1);
        assert_eq!(mails[0].subject, "Hello");
    }

    #[test]
    fn test_insert_mail_ignores_duplicate_uid_and_keeps_existing_row() {
        let conn = setup_db();
        let original = make_mail("m1", "<msg1@example.com>", "Original", "2026-04-13T10:00:00");
        assert!(insert_mail(&conn, &original).unwrap(), "初回は挿入される");

        // 同期の多重実行を模擬: 同じ (account, folder, uid) を別idで再挿入
        let mut duplicate =
            make_mail("m2", "<msg1@example.com>", "Duplicate", "2026-04-13T10:00:00");
        duplicate.uid = original.uid;
        assert!(!insert_mail(&conn, &duplicate).unwrap(), "重複は挿入されない");

        let mails = get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        assert_eq!(mails.len(), 1);
        assert_eq!(mails[0].id, "m1", "既存行が残る（REPLACEで消さない）");
        assert_eq!(mails[0].subject, "Original");
    }

    #[test]
    fn test_insert_mail_persists_is_read() {
        let conn = setup_db();
        let mut read_mail = make_mail("m1", "<msg1@example.com>", "Read", "2026-07-12T10:00:00");
        read_mail.is_read = true;
        let unread_mail = make_mail("m2", "<msg2@example.com>", "Unread", "2026-07-12T11:00:00");
        insert_mail(&conn, &read_mail).unwrap();
        insert_mail(&conn, &unread_mail).unwrap();

        let mails = get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        let read = mails.iter().find(|m| m.id == "m1").unwrap();
        let unread = mails.iter().find(|m| m.id == "m2").unwrap();
        assert!(read.is_read);
        assert!(!unread.is_read);
    }

    #[test]
    fn test_insert_mail_persists_is_flagged() {
        let conn = setup_db();
        let mut flagged_mail =
            make_mail("m1", "<msg1@example.com>", "Flagged", "2026-07-12T10:00:00");
        flagged_mail.is_flagged = true;
        let plain_mail = make_mail("m2", "<msg2@example.com>", "Plain", "2026-07-12T11:00:00");
        insert_mail(&conn, &flagged_mail).unwrap();
        insert_mail(&conn, &plain_mail).unwrap();

        let mails = get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        let flagged = mails.iter().find(|m| m.id == "m1").unwrap();
        let plain = mails.iter().find(|m| m.id == "m2").unwrap();
        assert!(flagged.is_flagged);
        assert!(!plain.is_flagged);
    }

    #[test]
    fn test_update_flag_state_syncs_server_state() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@example.com>", "A", "2026-07-12T10:00:00");
        m1.uid = 101;
        let mut m2 = make_mail("m2", "<msg2@example.com>", "B", "2026-07-12T11:00:00");
        m2.uid = 102;
        m2.is_read = true;
        insert_mail(&conn, &m1).unwrap();
        insert_mail(&conn, &m2).unwrap();

        // サーバー側: uid=101 は既読+フラグ、uid=102 は未読+フラグなし（他クライアントでの変更を模擬）
        let state: HashMap<u32, (bool, bool)> =
            [(101, (true, true)), (102, (false, false))].into_iter().collect();
        let updated = update_flag_state(&conn, "acc1", "INBOX", &state).unwrap();
        assert_eq!(updated, 2);

        let mails = get_mails_by_account(&conn, "acc1", "INBOX").unwrap();
        let m1_after = mails.iter().find(|m| m.id == "m1").unwrap();
        let m2_after = mails.iter().find(|m| m.id == "m2").unwrap();
        assert!(m1_after.is_read);
        assert!(m1_after.is_flagged);
        assert!(!m2_after.is_read);
        assert!(!m2_after.is_flagged);
    }

    #[test]
    fn test_update_flag_state_skips_unchanged_and_unknown_uids() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@example.com>", "A", "2026-07-12T10:00:00");
        m1.uid = 101;
        m1.is_read = true;
        insert_mail(&conn, &m1).unwrap();

        // uid=101 は既に既読・未フラグで変更なし。uid=999 は DB に存在しない
        let state: HashMap<u32, (bool, bool)> =
            [(101, (true, false)), (999, (true, true))].into_iter().collect();
        let updated = update_flag_state(&conn, "acc1", "INBOX", &state).unwrap();
        assert_eq!(updated, 0, "変更のない行・未知の uid は更新されない");
    }

    #[test]
    fn test_update_flag_state_updates_flagged_only_change() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@example.com>", "A", "2026-07-12T10:00:00");
        m1.uid = 101;
        m1.is_read = true;
        insert_mail(&conn, &m1).unwrap();

        // is_read は変化なしだが is_flagged だけ変わるケースも更新対象になる
        let state: HashMap<u32, (bool, bool)> = [(101, (true, true))].into_iter().collect();
        let updated = update_flag_state(&conn, "acc1", "INBOX", &state).unwrap();
        assert_eq!(updated, 1);
        let mail = get_mail_by_id(&conn, "m1").unwrap();
        assert!(mail.is_flagged);
    }

    #[test]
    fn test_mark_read_updates_row_and_returns_folder_uid() {
        let conn = setup_db();
        let mut mail = make_mail("m1", "<msg1@example.com>", "A", "2026-07-12T10:00:00");
        mail.uid = 42;
        insert_mail(&conn, &mail).unwrap();

        let (folder, uid) = mark_read(&conn, "m1").unwrap();
        assert_eq!(folder, "INBOX");
        assert_eq!(uid, 42);

        let stored = get_mail_by_id(&conn, "m1").unwrap();
        assert!(stored.is_read);
    }

    #[test]
    fn test_mark_read_missing_mail_returns_not_found() {
        let conn = setup_db();
        let result = mark_read(&conn, "nonexistent");
        assert!(matches!(result, Err(AppError::MailNotFound(_))));
    }

    #[test]
    fn test_mark_unread_updates_row_and_returns_folder_uid() {
        let conn = setup_db();
        let mut mail = make_mail("m1", "<msg1@example.com>", "A", "2026-07-12T10:00:00");
        mail.uid = 42;
        mail.is_read = true;
        insert_mail(&conn, &mail).unwrap();

        let (folder, uid) = mark_unread(&conn, "m1").unwrap();
        assert_eq!(folder, "INBOX");
        assert_eq!(uid, 42);

        let stored = get_mail_by_id(&conn, "m1").unwrap();
        assert!(!stored.is_read);
    }

    #[test]
    fn test_mark_unread_missing_mail_returns_not_found() {
        let conn = setup_db();
        let result = mark_unread(&conn, "nonexistent");
        assert!(matches!(result, Err(AppError::MailNotFound(_))));
    }

    #[test]
    fn test_set_flagged_updates_row_and_returns_folder_uid() {
        let conn = setup_db();
        let mut mail = make_mail("m1", "<msg1@example.com>", "A", "2026-07-12T10:00:00");
        mail.uid = 42;
        insert_mail(&conn, &mail).unwrap();

        let (folder, uid) = set_flagged(&conn, "m1", true).unwrap();
        assert_eq!(folder, "INBOX");
        assert_eq!(uid, 42);
        assert!(get_mail_by_id(&conn, "m1").unwrap().is_flagged);

        set_flagged(&conn, "m1", false).unwrap();
        assert!(!get_mail_by_id(&conn, "m1").unwrap().is_flagged);
    }

    #[test]
    fn test_set_flagged_missing_mail_returns_not_found() {
        let conn = setup_db();
        let result = set_flagged(&conn, "nonexistent", true);
        assert!(matches!(result, Err(AppError::MailNotFound(_))));
    }

    #[test]
    fn test_delete_mail_removes_row() {
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@example.com>", "Bye", "2026-07-12T10:00:00");
        insert_mail(&conn, &mail).unwrap();

        delete_mail(&conn, "m1").unwrap();

        assert!(matches!(
            get_mail_by_id(&conn, "m1"),
            Err(AppError::MailNotFound(_))
        ));
    }

    #[test]
    fn test_delete_mail_cascades_project_assignment() {
        use crate::db::{assignments, projects};
        use crate::models::project::CreateProjectRequest;

        let conn = setup_db();
        // CASCADE の検証には外部キーの有効化が必要（本番は lib.rs で有効化している）
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        let mail = make_mail("m1", "<msg1@example.com>", "Deal", "2026-07-12T10:00:00");
        insert_mail(&conn, &mail).unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "user", None).unwrap();

        delete_mail(&conn, "m1").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 'm1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "割り当ては CASCADE で消える");
    }

    #[test]
    fn test_delete_mail_missing_returns_not_found() {
        let conn = setup_db();
        assert!(matches!(
            delete_mail(&conn, "nonexistent"),
            Err(AppError::MailNotFound(_))
        ));
    }

    #[test]
    fn test_update_folder_moves_mail() {
        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@example.com>", "Keep", "2026-07-12T10:00:00");
        insert_mail(&conn, &mail).unwrap();

        update_folder(&conn, "m1", "Archive").unwrap();

        let stored = get_mail_by_id(&conn, "m1").unwrap();
        assert_eq!(stored.folder, "Archive");
        // INBOX の一覧からは消える
        assert!(get_mails_by_account(&conn, "acc1", "INBOX")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_update_folder_keeps_project_assignment() {
        use crate::db::{assignments, projects};
        use crate::models::project::CreateProjectRequest;

        let conn = setup_db();
        let mail = make_mail("m1", "<msg1@example.com>", "Deal", "2026-07-12T10:00:00");
        insert_mail(&conn, &mail).unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "user", None).unwrap();

        update_folder(&conn, "m1", "Archive").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 'm1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "アーカイブでは案件割り当てが維持される");
    }

    #[test]
    fn test_update_folder_missing_returns_not_found() {
        let conn = setup_db();
        assert!(matches!(
            update_folder(&conn, "nonexistent", "Archive"),
            Err(AppError::MailNotFound(_))
        ));
    }

    #[test]
    fn test_get_unread_counts_groups_by_project_and_unclassified() {
        use crate::db::{assignments, projects};
        use crate::models::project::CreateProjectRequest;

        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();

        // 案件に未読2・既読1、未分類に未読1、Sent（対象外）に未読1
        let mut mails_data = vec![
            ("m1", false, Some(proj.id.clone()), "INBOX"),
            ("m2", false, Some(proj.id.clone()), "INBOX"),
            ("m3", true, Some(proj.id.clone()), "INBOX"),
            ("m4", false, None, "INBOX"),
            ("m5", false, None, "Sent"),
        ];
        for (id, is_read, project_id, folder) in mails_data.drain(..) {
            let mut mail = make_mail(
                id,
                &format!("<{}@example.com>", id),
                "S",
                "2026-07-12T10:00:00",
            );
            mail.is_read = is_read;
            mail.folder = folder.into();
            insert_mail(&conn, &mail).unwrap();
            if let Some(pid) = project_id {
                assignments::assign_mail(&conn, id, &pid, "user", None).unwrap();
            }
        }

        let counts = get_unread_counts(&conn, "acc1").unwrap();
        assert_eq!(counts.by_project.get(&proj.id), Some(&2));
        assert_eq!(counts.unclassified, 1, "Sent の未読は数えない");
    }

    #[test]
    fn test_get_unread_counts_empty() {
        let conn = setup_db();
        let counts = get_unread_counts(&conn, "acc1").unwrap();
        assert!(counts.by_project.is_empty());
        assert_eq!(counts.unclassified, 0);
    }

    #[test]
    fn test_get_mail_id_by_message_id_matches_account_folder() {
        let conn = setup_db();
        let mut sent = make_mail("s1", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        sent.folder = "Sent".into();
        insert_mail(&conn, &sent).unwrap();

        // 一致
        assert_eq!(
            get_mail_id_by_message_id(&conn, "acc1", "Sent", "<mid@pigeon.local>").unwrap(),
            Some("s1".to_string())
        );
        // folder 違い・message_id 違いは None
        assert_eq!(
            get_mail_id_by_message_id(&conn, "acc1", "INBOX", "<mid@pigeon.local>").unwrap(),
            None
        );
        assert_eq!(
            get_mail_id_by_message_id(&conn, "acc1", "Sent", "<other@pigeon.local>").unwrap(),
            None
        );
    }

    #[test]
    fn test_confirm_uid_changes_uid_sets_confirmed_and_keeps_assignment() {
        use crate::db::{assignments, projects};
        use crate::models::project::CreateProjectRequest;

        let conn = setup_db();
        let mut sent = make_mail("s1", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        sent.folder = "Sent".into();
        sent.uid = 1; // 送信時の推定値
        sent.uid_confirmed = false;
        insert_mail(&conn, &sent).unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "s1", &proj.id, "user", None).unwrap();

        // サーバー同期で得た正しい uid に確定
        confirm_uid(&conn, "s1", 4242).unwrap();

        let stored = get_mail_by_id(&conn, "s1").unwrap();
        assert_eq!(stored.uid, 4242);
        assert!(stored.uid_confirmed, "確定フラグが立つ");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "uid 確定では案件割り当てが維持される");
    }

    #[test]
    fn test_confirm_uid_missing_returns_not_found() {
        let conn = setup_db();
        assert!(matches!(
            confirm_uid(&conn, "nonexistent", 1),
            Err(AppError::MailNotFound(_))
        ));
    }

    #[test]
    fn test_upsert_sent_mail_updates_existing_uid_no_duplicate() {
        use crate::db::{assignments, projects};
        use crate::models::project::CreateProjectRequest;

        let conn = setup_db();
        // 送信時ローカル保存分（推定 uid=1・未確定）
        let mut local = make_mail("s1", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        local.folder = "Sent".into();
        local.uid = 1;
        local.uid_confirmed = false;
        insert_mail(&conn, &local).unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "s1", &proj.id, "user", None).unwrap();

        // サーバー同期分（別 id・正しい uid=5000・同 message_id・衝突なし）
        let mut server = make_mail("s2", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        server.folder = "Sent".into();
        server.uid = 5000;
        let inserted = upsert_sent_mail(&conn, &server).unwrap();
        assert!(!inserted, "既存 message_id は新規挿入しない");

        // 行は1つのまま、uid はサーバー値に確定、案件割り当ては保持
        let all = get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(all.len(), 1, "二重行が作られない");
        assert_eq!(all[0].id, "s1", "既存行が残る");
        assert_eq!(all[0].uid, 5000, "uid がサーバー値に確定される");
        assert!(all[0].uid_confirmed, "確定フラグが立つ");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "案件割り当てが維持される");
    }

    #[test]
    fn test_upsert_sent_mail_merges_duplicate_on_uid_collision_keeping_assignment() {
        // C2: 確定先の uid が同一メールの重複行に占有されている場合、
        // 割り当てを持つ側を残して統合し、バッチを中断しない
        use crate::db::{assignments, projects};
        use crate::models::project::CreateProjectRequest;

        let conn = setup_db();
        // 送信時ローカル行（推定 uid=1・未確定・案件割り当てあり）
        let mut local = make_mail("s1", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        local.folder = "Sent".into();
        local.uid = 1;
        local.uid_confirmed = false;
        insert_mail(&conn, &local).unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "s1", &proj.id, "user", None).unwrap();

        // 同一メールの重複行が確定先 uid=5000 を既に占有（過去の部分同期分を模擬）
        let mut occupant = make_mail("s_dup", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        occupant.folder = "Sent".into();
        occupant.uid = 5000;
        occupant.uid_confirmed = true;
        insert_mail(&conn, &occupant).unwrap();

        // サーバー同期で uid=5000 を s1 に確定しようとする → 重複統合
        let mut server = make_mail("s_srv", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        server.folder = "Sent".into();
        server.uid = 5000;
        let inserted = upsert_sent_mail(&conn, &server).unwrap();
        assert!(!inserted);

        // 割り当てを持つ s1 が残り uid=5000 に確定、占有行 s_dup は削除、案件割り当ては生存
        let all = get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(all.len(), 1, "重複が統合されて1行");
        assert_eq!(all[0].id, "s1", "割り当てを持つ側が残る");
        assert_eq!(all[0].uid, 5000);
        assert!(all[0].uid_confirmed);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "案件割り当ては失われない");
    }

    #[test]
    fn test_upsert_sent_mail_skips_on_foreign_uid_collision_without_abort() {
        // C2: 確定先 uid を「別メール」が占有 → スキップして Ok を返し、バッチを継続させる
        let conn = setup_db();
        // 送信時ローカル行（推定 uid=1・未確定）
        let mut local = make_mail("s1", "<mine@pigeon.local>", "自分", "2026-07-12T10:00:00");
        local.folder = "Sent".into();
        local.uid = 1;
        local.uid_confirmed = false;
        insert_mail(&conn, &local).unwrap();
        // 別メールが確定先 uid=5000 を占有
        let mut foreign = make_mail("s_foreign", "<foreign@server>", "他人", "2026-07-12T09:00:00");
        foreign.folder = "Sent".into();
        foreign.uid = 5000;
        insert_mail(&conn, &foreign).unwrap();

        // s1 を uid=5000 に確定しようとするが別メール占有 → スキップ（Err にしない）
        let mut server = make_mail("s_srv", "<mine@pigeon.local>", "自分", "2026-07-12T10:00:00");
        server.folder = "Sent".into();
        server.uid = 5000;
        let result = upsert_sent_mail(&conn, &server);
        assert!(result.is_ok(), "衝突でも Err にせずバッチを継続できる");
        assert!(!result.unwrap());

        // s1 は uid=1・未確定のまま（誤って別メールの uid を奪わない）、占有行も無傷
        let s1 = get_mail_by_id(&conn, "s1").unwrap();
        assert_eq!(s1.uid, 1);
        assert!(!s1.uid_confirmed);
        let foreign_row = get_mail_by_id(&conn, "s_foreign").unwrap();
        assert_eq!(foreign_row.uid, 5000);
    }

    #[test]
    fn test_upsert_sent_mail_inserts_new_message_id() {
        // 他クライアントから送ったメール（ローカルに送信行が無い）は新規取り込みされる
        let conn = setup_db();
        let mut server = make_mail("s1", "<external@server>", "他クライアント", "2026-07-12T10:00:00");
        server.folder = "Sent".into();
        server.uid = 777;
        let inserted = upsert_sent_mail(&conn, &server).unwrap();
        assert!(inserted, "未知の message_id は挿入される");

        let all = get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].uid, 777);
    }

    #[test]
    fn test_get_recent_unread_subjects_orders_by_date_desc_and_limits() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<m1@example.com>", "Oldest", "2026-07-12T09:00:00");
        m1.is_read = false;
        let mut m2 = make_mail("m2", "<m2@example.com>", "Middle", "2026-07-12T10:00:00");
        m2.is_read = false;
        let mut m3 = make_mail("m3", "<m3@example.com>", "Newest", "2026-07-12T11:00:00");
        m3.is_read = false;
        insert_mail(&conn, &m1).unwrap();
        insert_mail(&conn, &m2).unwrap();
        insert_mail(&conn, &m3).unwrap();

        let subjects = get_recent_unread_subjects(&conn, "acc1", 2).unwrap();
        assert_eq!(subjects, vec!["Newest".to_string(), "Middle".to_string()]);
    }

    #[test]
    fn test_get_recent_unread_subjects_excludes_read_and_other_folders() {
        let conn = setup_db();
        let mut read_mail = make_mail("m1", "<m1@example.com>", "Read", "2026-07-12T10:00:00");
        read_mail.is_read = true;
        let mut sent_mail = make_mail("m2", "<m2@example.com>", "Sent", "2026-07-12T11:00:00");
        sent_mail.is_read = false;
        sent_mail.folder = "Sent".into();
        let mut unread = make_mail("m3", "<m3@example.com>", "Unread", "2026-07-12T12:00:00");
        unread.is_read = false;
        insert_mail(&conn, &read_mail).unwrap();
        insert_mail(&conn, &sent_mail).unwrap();
        insert_mail(&conn, &unread).unwrap();

        let subjects = get_recent_unread_subjects(&conn, "acc1", 10).unwrap();
        assert_eq!(subjects, vec!["Unread".to_string()]);
    }

    #[test]
    fn test_get_recent_unread_subjects_empty() {
        let conn = setup_db();
        let subjects = get_recent_unread_subjects(&conn, "acc1", 3).unwrap();
        assert!(subjects.is_empty());
    }

    #[test]
    fn test_get_max_uid() {
        let conn = setup_db();
        assert_eq!(get_max_uid(&conn, "acc1", "INBOX").unwrap(), 0);
        let mut mail = make_mail("m1", "<msg1@example.com>", "Test", "2026-04-13T10:00:00");
        mail.uid = 42;
        insert_mail(&conn, &mail).unwrap();
        assert_eq!(get_max_uid(&conn, "acc1", "INBOX").unwrap(), 42);
    }

    #[test]
    fn test_get_min_uid() {
        // バックフィルの起点（ここより古いUIDをサーバーへ問い合わせる）
        let conn = setup_db();
        assert_eq!(get_min_uid(&conn, "acc1", "INBOX").unwrap(), 0);
        let mut m1 = make_mail("m1", "<msg1@example.com>", "Test1", "2026-04-13T10:00:00");
        m1.uid = 42;
        insert_mail(&conn, &m1).unwrap();
        let mut m2 = make_mail("m2", "<msg2@example.com>", "Test2", "2026-04-13T11:00:00");
        m2.uid = 10;
        insert_mail(&conn, &m2).unwrap();
        assert_eq!(get_min_uid(&conn, "acc1", "INBOX").unwrap(), 10);
    }

    #[test]
    fn test_get_max_confirmed_uid_ignores_unconfirmed_rows() {
        // C1 の核心: 送信時の推定 uid（未確定・大きい値）が watermark を汚染しないこと
        let conn = setup_db();
        // 確定済みの同期行（サーバー実 uid=100）
        let mut confirmed = make_mail("s1", "<a@ex.com>", "S1", "2026-07-12T10:00:00");
        confirmed.folder = "Sent".into();
        confirmed.uid = 100;
        confirmed.uid_confirmed = true;
        insert_mail(&conn, &confirmed).unwrap();
        // 送信時の推定 uid（未確定・実 uid より大きい 9999）
        let mut estimated = make_mail("s2", "<b@ex.com>", "S2", "2026-07-12T11:00:00");
        estimated.folder = "Sent".into();
        estimated.uid = 9999;
        estimated.uid_confirmed = false;
        insert_mail(&conn, &estimated).unwrap();

        // get_max_uid は推定値も含めてしまう（汚染された watermark）
        assert_eq!(get_max_uid(&conn, "acc1", "Sent").unwrap(), 9999);
        // get_max_confirmed_uid は確定行のみ → 実 uid が 100 以上のサーバー行を取りこぼさない
        assert_eq!(get_max_confirmed_uid(&conn, "acc1", "Sent").unwrap(), 100);
    }

    #[test]
    fn test_get_max_confirmed_uid_zero_when_all_unconfirmed() {
        let conn = setup_db();
        let mut estimated = make_mail("s1", "<b@ex.com>", "S", "2026-07-12T10:00:00");
        estimated.folder = "Sent".into();
        estimated.uid = 5000;
        estimated.uid_confirmed = false;
        insert_mail(&conn, &estimated).unwrap();
        // 確定行が皆無なら 0 → 初回 Sent 同期として全件対象になる
        assert_eq!(get_max_confirmed_uid(&conn, "acc1", "Sent").unwrap(), 0);
    }

    #[test]
    fn test_build_threads_by_in_reply_to() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Hello", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_by_references() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.references = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail(
            "m3",
            "<msg3@ex.com>",
            "Re: Re: Topic",
            "2026-04-13T12:00:00",
        );
        m3.references = Some("<msg1@ex.com> <msg2@ex.com>".into());
        let threads = build_threads(&[m1, m2, m3]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 3);
    }

    #[test]
    fn test_build_threads_subject_fallback() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "見積もりの件", "2026-04-13T10:00:00");
        let m2 = make_mail(
            "m2",
            "<msg2@ex.com>",
            "Re: 見積もりの件",
            "2026-04-13T11:00:00",
        );
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_separate() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic A", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Topic B", "2026-04-13T11:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Re: Hello"), "hello");
        assert_eq!(normalize_subject("Fwd: Re: Hello"), "hello");
        assert_eq!(normalize_subject("FW: Hello"), "hello");
        assert_eq!(normalize_subject("Hello"), "hello");
    }

    #[test]
    fn test_build_threads_empty() {
        let threads = build_threads(&[]);
        assert!(threads.is_empty());
    }

    #[test]
    fn test_build_threads_single_mail() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Solo", "2026-04-13T10:00:00");
        let threads = build_threads(&[m1]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 1);
        assert_eq!(threads[0].subject, "Solo");
    }

    #[test]
    fn test_build_threads_sorted_by_last_date_desc() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Old Topic", "2026-04-10T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "New Topic", "2026-04-13T10:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].subject, "New Topic");
        assert_eq!(threads[1].subject, "Old Topic");
    }

    #[test]
    fn test_build_threads_fw_prefix_groups() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "案件の件", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Fw: 案件の件", "2026-04-13T11:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_fwd_prefix_groups() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Report", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Fwd: Report", "2026-04-13T11:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
    }

    #[test]
    fn test_build_threads_deep_chain() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail(
            "m3",
            "<msg3@ex.com>",
            "Re: Re: Topic",
            "2026-04-13T12:00:00",
        );
        m3.in_reply_to = Some("<msg2@ex.com>".into());
        let mut m4 = make_mail(
            "m4",
            "<msg4@ex.com>",
            "Re: Re: Re: Topic",
            "2026-04-13T13:00:00",
        );
        m4.in_reply_to = Some("<msg3@ex.com>".into());
        let threads = build_threads(&[m1, m2, m3, m4]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 4);
    }

    #[test]
    fn test_build_threads_from_addrs_deduplication() {
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        m1.from_addr = "alice@example.com".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.from_addr = "alice@example.com".into();
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads[0].from_addrs.len(), 1);
    }

    #[test]
    fn test_build_threads_subject_grouping_skipped_when_has_references() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Same Subject", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Same Subject", "2026-04-13T11:00:00");
        m2.references = Some("<nonexistent@ex.com>".into());
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_normalize_subject_nested_prefixes() {
        assert_eq!(normalize_subject("Re: Fw: Re: Hello"), "hello");
        assert_eq!(normalize_subject("FW: FWD: RE: Hello"), "hello");
    }

    #[test]
    fn test_normalize_subject_case_insensitive() {
        assert_eq!(normalize_subject("RE: HELLO"), "hello");
        assert_eq!(normalize_subject("re: hello"), "hello");
    }

    #[test]
    fn test_normalize_subject_whitespace() {
        assert_eq!(normalize_subject("  Re:   Hello  "), "hello");
    }
}
