use crate::db::assignments;
use crate::error::AppError;
use crate::models::mail::{Mail, Thread, UnreadCounts};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::LazyLock;

// スレッド判定アルゴリズムは DB 非依存のドメインロジックとして
// `crate::threading` に分離した。既存呼び出し側（`db::mails::group_mail_ids_into_threads`
// 等）の互換のためここから再エクスポートする。
pub use crate::threading::{group_mail_ids_into_threads, ThreadMailMeta};

// Sent マージの業務ロジック（uid の仮採番・後追い確定・重複統合）は
// `db::sent_sync` に分離した。既存呼び出し側（`db::mails::upsert_sent_mail` 等）の
// 互換のためここから再エクスポートする。
pub use crate::db::sent_sync::{insert_sent_mail_with_next_uid, upsert_sent_mail};

/// 互換ラッパー: 実体は `crate::threading::build_threads`（所有権を取る版）。
/// 借用スライスしか持たない既存の commands 層呼び出しのためにクローンして委譲する。
/// 所有権を渡せる呼び出し側は `crate::threading::build_threads` を直接使うこと。
pub fn build_threads(mails: &[Mail]) -> Vec<Thread> {
    crate::threading::build_threads(mails.to_vec())
}

/// mails テーブルのカラム名の唯一の定義。
/// SELECT 句（`MAIL_COLUMNS` / `MAIL_COLUMNS_PREFIXED`）・INSERT 句・
/// `MAIL_COLUMN_COUNT` はすべてここから導出する。カラム追加時は
/// この配列と `row_to_mail`（と INSERT の VALUES）だけを同期すればよい。
pub const MAIL_COLUMN_NAMES: &[&str] = &[
    "id",
    "account_id",
    "folder",
    "message_id",
    "in_reply_to",
    "\"references\"",
    "from_addr",
    "to_addr",
    "cc_addr",
    "subject",
    "body_text",
    "body_html",
    "date",
    "has_attachments",
    "raw_size",
    "uid",
    "flags",
    "is_read",
    "is_flagged",
    "fetched_at",
    "uid_confirmed",
];

/// Number of columns in MAIL_COLUMNS / MAIL_COLUMNS_PREFIXED.
/// JOIN クエリで追加カラムを読む際のオフセットとして使う。
pub const MAIL_COLUMN_COUNT: usize = MAIL_COLUMN_NAMES.len();

/// Column list for SELECT queries on the mails table (no table prefix).
/// Must match the field order expected by `row_to_mail`.
pub static MAIL_COLUMNS: LazyLock<String> = LazyLock::new(|| MAIL_COLUMN_NAMES.join(", "));

/// Column list with `m.` table prefix for JOIN queries.
pub static MAIL_COLUMNS_PREFIXED: LazyLock<String> = LazyLock::new(|| {
    MAIL_COLUMN_NAMES
        .iter()
        .map(|c| format!("m.{}", c))
        .collect::<Vec<_>>()
        .join(", ")
});

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
        &format!("SELECT {} FROM mails WHERE id = ?1", *MAIL_COLUMNS),
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
    // 挿入と FTS 索引の2文を原子化する（索引失敗時に検索不能な行を残さない）
    crate::db::tx::with_tx(conn, |conn| insert_mail_inner(conn, mail))
}

fn insert_mail_inner(conn: &Connection, mail: &Mail) -> Result<bool, AppError> {
    let affected = conn.execute(
        &format!(
            "INSERT OR IGNORE INTO mails ({})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            *MAIL_COLUMNS
        ),
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
    let inserted = affected > 0;
    if inserted {
        crate::db::fts::index_mail(conn, mail)?;
    }
    Ok(inserted)
}

pub fn get_mails_by_account(
    conn: &Connection,
    account_id: &str,
    folder: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM mails WHERE account_id = ?1 AND folder = ?2 ORDER BY date DESC",
        *MAIL_COLUMNS
    ))?;
    let mails = stmt
        .query_map(params![account_id, folder], row_to_mail)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mails)
}

pub fn get_max_uid(conn: &Connection, account_id: &str, folder: &str) -> Result<u32, AppError> {
    // 集計クエリは常に1行返す（行なしは COALESCE で 0）。エラーは同期 watermark を
    // 誤って 0 に丸めないようそのまま伝播する（B-10）
    let uid: u32 = conn.query_row(
        "SELECT COALESCE(MAX(uid), 0) FROM mails WHERE account_id = ?1 AND folder = ?2",
        params![account_id, folder],
        |row| row.get(0),
    )?;
    Ok(uid)
}

/// folder 内の最小 uid を返す（行が無ければ 0）。バックフィルの起点
/// （ここ未満の UID をサーバーへ遡って問い合わせる）に使う。
pub fn get_min_uid(conn: &Connection, account_id: &str, folder: &str) -> Result<u32, AppError> {
    let uid: u32 = conn.query_row(
        "SELECT COALESCE(MIN(uid), 0) FROM mails WHERE account_id = ?1 AND folder = ?2",
        params![account_id, folder],
        |row| row.get(0),
    )?;
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
    let uid: u32 = conn.query_row(
        "SELECT COALESCE(MAX(uid), 0) FROM mails
         WHERE account_id = ?1 AND folder = ?2 AND uid_confirmed = 1",
        params![account_id, folder],
        |row| row.get(0),
    )?;
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
/// correction_log は CASCADE で消え、FTS は db::fts::remove_mail で同期する。
/// 対象が存在しなければ MailNotFound。
pub fn delete_mail(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    // 行削除と FTS 索引削除の2文を原子化する
    crate::db::tx::with_tx(conn, |conn| {
        let affected = conn.execute("DELETE FROM mails WHERE id = ?1", params![mail_id])?;
        if affected == 0 {
            return Err(AppError::MailNotFound(mail_id.to_string()));
        }
        crate::db::fts::remove_mail(conn, mail_id)?;
        Ok(())
    })
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
        .optional()?;
    Ok(id)
}

/// mails の bool 1カラムを更新し、サーバー反映に必要な (folder, uid) を返す共通処理
/// （mark_read / mark_unread / set_flagged の本体）。対象が存在しなければ MailNotFound。
/// `column` は SQL に直接埋め込むため、このモジュール内の固定リテラルのみ渡すこと。
fn set_mail_bool_column(
    conn: &Connection,
    mail_id: &str,
    column: &str,
    value: bool,
) -> Result<(String, u32), AppError> {
    let (folder, uid): (String, u32) = conn
        .query_row(
            "SELECT folder, uid FROM mails WHERE id = ?1",
            params![mail_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::MailNotFound(mail_id.to_string()))?;
    conn.execute(
        &format!("UPDATE mails SET {} = ?1 WHERE id = ?2", column),
        params![value, mail_id],
    )?;
    Ok((folder, uid))
}

/// メールを既読にし、サーバー反映に必要な (folder, uid) を返す。
pub fn mark_read(conn: &Connection, mail_id: &str) -> Result<(String, u32), AppError> {
    set_mail_bool_column(conn, mail_id, "is_read", true)
}

/// メールを未読に戻し、サーバー反映に必要な (folder, uid) を返す（mark_read の逆）。
pub fn mark_unread(conn: &Connection, mail_id: &str) -> Result<(String, u32), AppError> {
    set_mail_bool_column(conn, mail_id, "is_read", false)
}

/// メールのスター/フラグを設定し、サーバー反映に必要な (folder, uid) を返す。
pub fn set_flagged(
    conn: &Connection,
    mail_id: &str,
    flagged: bool,
) -> Result<(String, u32), AppError> {
    set_mail_bool_column(conn, mail_id, "is_flagged", flagged)
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
        .collect::<rusqlite::Result<Vec<_>>>()?;
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
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;

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

pub fn get_threads_by_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<Thread>, AppError> {
    let mails = assignments::get_mails_by_project(conn, project_id)?;
    // 所有権を渡してクローンなしでスレッドへ組み立てる
    Ok(crate::threading::build_threads(mails))
}

/// アカウントの全フォルダのメールをスレッド判定用の軽量メタとして返す（date DESC）。
/// スレッド判定には Sent/Archive 等、INBOX 以外のメールもリンクの手がかりとして
/// 必要なためフォルダ横断で取得する。本文カラム（body_text/body_html）は読まない。
pub fn get_thread_metas_by_account(
    conn: &Connection,
    account_id: &str,
) -> Result<Vec<ThreadMailMeta>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, message_id, in_reply_to, \"references\", subject, date
         FROM mails WHERE account_id = ?1 ORDER BY date DESC",
    )?;
    let metas = stmt
        .query_map(params![account_id], |row| {
            Ok(ThreadMailMeta {
                id: row.get(0)?,
                message_id: row.get(1)?,
                in_reply_to: row.get(2)?,
                references: row.get(3)?,
                subject: row.get(4)?,
                date: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(metas)
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
        let original = make_mail(
            "m1",
            "<msg1@example.com>",
            "Original",
            "2026-04-13T10:00:00",
        );
        assert!(insert_mail(&conn, &original).unwrap(), "初回は挿入される");

        // 同期の多重実行を模擬: 同じ (account, folder, uid) を別idで再挿入
        let mut duplicate = make_mail(
            "m2",
            "<msg1@example.com>",
            "Duplicate",
            "2026-04-13T10:00:00",
        );
        duplicate.uid = original.uid;
        assert!(
            !insert_mail(&conn, &duplicate).unwrap(),
            "重複は挿入されない"
        );

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
        let state: HashMap<u32, (bool, bool)> = [(101, (true, true)), (102, (false, false))]
            .into_iter()
            .collect();
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
        let state: HashMap<u32, (bool, bool)> = [(101, (true, false)), (999, (true, true))]
            .into_iter()
            .collect();
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
    fn test_get_max_uid_propagates_real_errors() {
        // B-10: 「行なし」は Ok(0) だが、DB 破損等の実エラーを 0 に丸めてはいけない。
        // watermark が誤って 0 になると全件再取得や取りこぼしを誘発するため Err で伝播する
        let conn = setup_db();
        assert_eq!(
            get_max_uid(&conn, "acc1", "INBOX").unwrap(),
            0,
            "行なしは Ok(0)"
        );

        // 実エラーを注入: mails テーブル自体を破壊する
        conn.execute_batch("DROP TABLE mails").unwrap();
        assert!(
            get_max_uid(&conn, "acc1", "INBOX").is_err(),
            "実エラーは 0 に丸めず Err で伝播する"
        );
        assert!(get_min_uid(&conn, "acc1", "INBOX").is_err());
        assert!(get_max_confirmed_uid(&conn, "acc1", "INBOX").is_err());
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
    fn test_get_thread_metas_by_account_spans_folders_and_orders_by_date_desc() {
        // スレッド追従の判定には INBOX 以外（Sent/Archive）のメールも手がかりとして
        // 必要なため、フォルダ横断・date DESC で軽量メタを返すこと
        let conn = setup_db();
        let inbox = make_mail(
            "m1",
            "<msg1@example.com>",
            "Inbox Mail",
            "2026-04-13T10:00:00",
        );
        let mut sent = make_mail(
            "m2",
            "<msg2@example.com>",
            "Sent Mail",
            "2026-04-13T11:00:00",
        );
        sent.folder = "Sent".into();
        sent.in_reply_to = Some("<msg1@example.com>".into());
        sent.references = Some("<msg0@example.com> <msg1@example.com>".into());
        let mut archived = make_mail(
            "m3",
            "<msg3@example.com>",
            "Archived Mail",
            "2026-04-13T09:00:00",
        );
        archived.folder = "Archive".into();
        insert_mail(&conn, &inbox).unwrap();
        insert_mail(&conn, &sent).unwrap();
        insert_mail(&conn, &archived).unwrap();

        let metas = get_thread_metas_by_account(&conn, "acc1").unwrap();
        assert_eq!(metas.len(), 3, "全フォルダのメールを対象にする");
        // date DESC 順
        assert_eq!(metas[0].id, "m2");
        assert_eq!(metas[1].id, "m1");
        assert_eq!(metas[2].id, "m3");
        // スレッド判定に必要なカラムが揃っていること
        assert_eq!(metas[0].message_id, "<msg2@example.com>");
        assert_eq!(metas[0].in_reply_to, Some("<msg1@example.com>".to_string()));
        assert_eq!(
            metas[0].references,
            Some("<msg0@example.com> <msg1@example.com>".to_string())
        );
        assert_eq!(metas[0].subject, "Sent Mail");
        assert_eq!(metas[0].date, "2026-04-13T11:00:00");
    }

    #[test]
    fn test_get_thread_metas_by_account_excludes_other_accounts() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        )
        .unwrap();
        let mine = make_mail("m1", "<msg1@example.com>", "Mine", "2026-04-13T10:00:00");
        let mut theirs = make_mail("m2", "<msg2@example.com>", "Theirs", "2026-04-13T10:00:00");
        theirs.account_id = "acc2".into();
        insert_mail(&conn, &mine).unwrap();
        insert_mail(&conn, &theirs).unwrap();

        let metas = get_thread_metas_by_account(&conn, "acc1").unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, "m1");
    }
}
