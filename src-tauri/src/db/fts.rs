//! fts_mails 索引の書き込みを一元管理する。
//! v17 で SQL トリガー同期を廃止したため、mails への書き込みは必ずこの
//! モジュール経由で FTS を同期すること。現在の呼び出し元:
//! insert_mail / delete_mail / delete_account /
//! sent_sync::insert_sent_mail_with_next_uid（送信メールのローカル Sent 保存）。
//! 索引には search_normalize::normalize_for_search を適用した正規化済み
//! テキストを格納する（クエリ側も同じ正規化を適用して照合する）。

use crate::error::AppError;
use crate::models::mail::Mail;
use crate::search_normalize::normalize_for_search;
use rusqlite::{params, Connection};

pub fn index_mail(conn: &Connection, mail: &Mail) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            mail.id,
            normalize_for_search(&mail.subject),
            normalize_for_search(mail.body_text.as_deref().unwrap_or("")),
            normalize_for_search(&mail.from_addr),
            normalize_for_search(&mail.to_addr),
        ],
    )?;
    Ok(())
}

pub fn remove_mail(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM fts_mails WHERE mail_id = ?1", params![mail_id])?;
    Ok(())
}

pub fn remove_account_mails(conn: &Connection, account_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM fts_mails
         WHERE mail_id IN (SELECT id FROM mails WHERE account_id = ?1)",
        params![account_id],
    )?;
    Ok(())
}

pub fn rebuild(conn: &Connection) -> Result<usize, AppError> {
    conn.execute("DELETE FROM fts_mails", [])?;
    let mut stmt = conn.prepare("SELECT id, subject, body_text, from_addr, to_addr FROM mails")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut count = 0usize;
    let mut insert = conn.prepare(
        "INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for row in rows {
        let (id, subject, body_text, from_addr, to_addr) = row?;
        insert.execute(params![
            id,
            normalize_for_search(&subject),
            normalize_for_search(body_text.as_deref().unwrap_or("")),
            normalize_for_search(&from_addr),
            normalize_for_search(&to_addr),
        ])?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{accounts, mails, sent_sync};
    use crate::test_helpers::{make_mail, setup_db};

    fn fts_row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM fts_mails", [], |r| r.get(0))
            .unwrap()
    }

    fn fts_subject(conn: &Connection, mail_id: &str) -> String {
        conn.query_row(
            "SELECT subject FROM fts_mails WHERE mail_id = ?1",
            [mail_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn test_no_fts_triggers_after_migrations() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'trigger' AND name LIKE 'trg_fts_mails%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "v17でFTSトリガーは廃止されているはず");
    }

    #[test]
    fn test_insert_mail_indexes_normalized_text() {
        let conn = setup_db();
        let m = make_mail(
            "m1",
            "<m1@ex.com>",
            "ＳＡＴＯ様 みつもり",
            "2026-07-17T10:00:00",
        );
        mails::insert_mail(&conn, &m).unwrap();
        assert_eq!(fts_row_count(&conn), 1);
        assert_eq!(fts_subject(&conn, "m1"), "sato様 ミツモリ");
    }

    #[test]
    fn test_insert_mail_ignored_duplicate_does_not_double_index() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        assert!(mails::insert_mail(&conn, &m).unwrap());
        // 同じ (account_id, folder, uid) は INSERT OR IGNORE で弾かれる
        let mut dup = make_mail("m2", "<m2@ex.com>", "Hello", "2026-07-17T10:00:00");
        dup.uid = m.uid;
        assert!(!mails::insert_mail(&conn, &dup).unwrap());
        assert_eq!(fts_row_count(&conn), 1);
    }

    #[test]
    fn test_delete_mail_removes_fts_row() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        mails::delete_mail(&conn, "m1").unwrap();
        assert_eq!(fts_row_count(&conn), 0);
    }

    #[test]
    fn test_delete_account_removes_fts_rows() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        accounts::delete_account(&conn, "acc1").unwrap();
        assert_eq!(fts_row_count(&conn), 0);
    }

    #[test]
    fn test_rebuild_reindexes_all_mails() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<m1@ex.com>", "ＡＢＣ", "2026-07-17T10:00:00");
        let m2 = make_mail("m2", "<m2@ex.com>", "かたかな", "2026-07-17T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();
        // 索引を壊してから rebuild で復元されることを確認
        conn.execute("DELETE FROM fts_mails", []).unwrap();
        let n = rebuild(&conn).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fts_subject(&conn, "m1"), "abc");
        assert_eq!(fts_subject(&conn, "m2"), "カタカナ");
    }

    // --- 挿入/削除と FTS 索引の原子性 ---

    fn mails_row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM mails", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn test_insert_mail_rolls_back_when_indexing_fails() {
        let conn = setup_db();
        // FTS テーブルを壊して index_mail を失敗させる
        conn.execute_batch("DROP TABLE fts_mails;").unwrap();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        assert!(mails::insert_mail(&conn, &m).is_err());
        assert_eq!(
            mails_row_count(&conn),
            0,
            "索引失敗時はメール挿入ごとロールバックされる（FTSに無い行を作らない）"
        );
    }

    #[test]
    fn test_delete_mail_rolls_back_when_index_removal_fails() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        conn.execute_batch("DROP TABLE fts_mails;").unwrap();
        assert!(mails::delete_mail(&conn, "m1").is_err());
        assert_eq!(
            mails_row_count(&conn),
            1,
            "索引削除失敗時は行削除ごとロールバックされる"
        );
    }

    #[test]
    fn test_insert_sent_mail_rolls_back_when_indexing_fails() {
        let conn = setup_db();
        conn.execute_batch("DROP TABLE fts_mails;").unwrap();
        let m = make_mail("m1", "<m1@ex.com>", "Sent mail", "2026-07-17T10:00:00");
        assert!(sent_sync::insert_sent_mail_with_next_uid(&conn, &m).is_err());
        assert_eq!(mails_row_count(&conn), 0);
    }

    #[test]
    fn test_insert_mail_inside_caller_transaction_still_works() {
        // 呼び出し元が既にトランザクションを張っているケース（upsert_sent_mail 等）で
        // ネスト BEGIN にならないことの回帰テスト
        let conn = setup_db();
        let tx = conn.unchecked_transaction().unwrap();
        let m = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-17T10:00:00");
        assert!(mails::insert_mail(&tx, &m).unwrap());
        mails::delete_mail(&tx, "m1").unwrap();
        assert!(mails::insert_mail(&tx, &m).unwrap());
        tx.commit().unwrap();
        assert_eq!(mails_row_count(&conn), 1);
        assert_eq!(fts_row_count(&conn), 1);
    }
}
