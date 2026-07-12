//! `drafts` テーブルのCRUD。
//! 設計: docs/superpowers/specs/2026-07-12-draft-save-design.md

use crate::error::AppError;
use crate::models::draft::Draft;
use rusqlite::{params, Connection};

const DRAFT_COLUMNS: &str = "id, account_id, to_addr, cc_addr, bcc_addr, subject, body_text,
     in_reply_to, created_at, updated_at";

fn row_to_draft(row: &rusqlite::Row<'_>) -> rusqlite::Result<Draft> {
    Ok(Draft {
        id: row.get(0)?,
        account_id: row.get(1)?,
        to_addr: row.get(2)?,
        cc_addr: row.get(3)?,
        bcc_addr: row.get(4)?,
        subject: row.get(5)?,
        body_text: row.get(6)?,
        in_reply_to: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

/// 下書きを新規作成する。id は呼び出し側が採番する。
pub fn insert_draft(conn: &Connection, draft: &Draft) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO drafts
         (id, account_id, to_addr, cc_addr, bcc_addr, subject, body_text, in_reply_to, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            draft.id,
            draft.account_id,
            draft.to_addr,
            draft.cc_addr,
            draft.bcc_addr,
            draft.subject,
            draft.body_text,
            draft.in_reply_to,
            draft.created_at,
            draft.updated_at,
        ],
    )?;
    Ok(())
}

/// 既存の下書きを上書きする（updated_at のみ呼び出し側の値で更新）。
/// 対象が存在しなければ DraftNotFound。
pub fn update_draft(conn: &Connection, draft: &Draft) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE drafts SET to_addr = ?1, cc_addr = ?2, bcc_addr = ?3, subject = ?4,
         body_text = ?5, in_reply_to = ?6, updated_at = ?7
         WHERE id = ?8",
        params![
            draft.to_addr,
            draft.cc_addr,
            draft.bcc_addr,
            draft.subject,
            draft.body_text,
            draft.in_reply_to,
            draft.updated_at,
            draft.id,
        ],
    )?;
    if affected == 0 {
        return Err(AppError::DraftNotFound(draft.id.clone()));
    }
    Ok(())
}

/// アカウントの下書き有無を確認する（upsert の分岐に使う）。
pub fn exists(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM drafts WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(exists)
}

/// アカウントの下書き一覧（updated_at 降順）。
pub fn get_drafts_by_account(conn: &Connection, account_id: &str) -> Result<Vec<Draft>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM drafts WHERE account_id = ?1 ORDER BY updated_at DESC",
        DRAFT_COLUMNS
    ))?;
    let drafts = stmt
        .query_map(params![account_id], row_to_draft)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(drafts)
}

/// 下書きを削除する。対象が存在しなくてもエラーにしない
/// （送信成功時の削除で「既に無い」を許容するため）。
pub fn delete_draft(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM drafts WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn make_draft(id: &str, account_id: &str, subject: &str, updated_at: &str) -> Draft {
        Draft {
            id: id.into(),
            account_id: account_id.into(),
            to_addr: "tanaka@example.com".into(),
            cc_addr: "".into(),
            bcc_addr: "".into(),
            subject: subject.into(),
            body_text: "本文".into(),
            in_reply_to: None,
            created_at: "2026-07-12T10:00:00Z".into(),
            updated_at: updated_at.into(),
        }
    }

    #[test]
    fn test_insert_and_get_draft() {
        let conn = setup_db();
        let draft = make_draft("d1", "acc1", "件名A", "2026-07-12T10:00:00Z");
        insert_draft(&conn, &draft).unwrap();

        let drafts = get_drafts_by_account(&conn, "acc1").unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].subject, "件名A");
        assert_eq!(drafts[0].to_addr, "tanaka@example.com");
    }

    #[test]
    fn test_exists_true_and_false() {
        let conn = setup_db();
        let draft = make_draft("d1", "acc1", "件名A", "2026-07-12T10:00:00Z");
        insert_draft(&conn, &draft).unwrap();

        assert!(exists(&conn, "d1").unwrap());
        assert!(!exists(&conn, "nonexistent").unwrap());
    }

    #[test]
    fn test_update_draft_overwrites_fields() {
        let conn = setup_db();
        let draft = make_draft("d1", "acc1", "旧件名", "2026-07-12T10:00:00Z");
        insert_draft(&conn, &draft).unwrap();

        let mut updated = draft.clone();
        updated.subject = "新件名".into();
        updated.updated_at = "2026-07-12T11:00:00Z".into();
        update_draft(&conn, &updated).unwrap();

        let drafts = get_drafts_by_account(&conn, "acc1").unwrap();
        assert_eq!(drafts.len(), 1, "update は行数を増やさない");
        assert_eq!(drafts[0].subject, "新件名");
        assert_eq!(drafts[0].updated_at, "2026-07-12T11:00:00Z");
    }

    #[test]
    fn test_update_draft_missing_returns_not_found() {
        let conn = setup_db();
        let draft = make_draft("nonexistent", "acc1", "S", "2026-07-12T10:00:00Z");
        let result = update_draft(&conn, &draft);
        assert!(matches!(result, Err(AppError::DraftNotFound(_))));
    }

    #[test]
    fn test_get_drafts_by_account_ordered_by_updated_at_desc() {
        let conn = setup_db();
        insert_draft(
            &conn,
            &make_draft("d1", "acc1", "Old", "2026-07-12T09:00:00Z"),
        )
        .unwrap();
        insert_draft(
            &conn,
            &make_draft("d2", "acc1", "New", "2026-07-12T11:00:00Z"),
        )
        .unwrap();
        insert_draft(
            &conn,
            &make_draft("d3", "acc1", "Mid", "2026-07-12T10:00:00Z"),
        )
        .unwrap();

        let drafts = get_drafts_by_account(&conn, "acc1").unwrap();
        let subjects: Vec<&str> = drafts.iter().map(|d| d.subject.as_str()).collect();
        assert_eq!(subjects, vec!["New", "Mid", "Old"]);
    }

    #[test]
    fn test_get_drafts_by_account_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'B', 'b@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        insert_draft(
            &conn,
            &make_draft("d1", "acc1", "A", "2026-07-12T10:00:00Z"),
        )
        .unwrap();
        insert_draft(
            &conn,
            &make_draft("d2", "acc2", "B", "2026-07-12T10:00:00Z"),
        )
        .unwrap();

        let drafts = get_drafts_by_account(&conn, "acc1").unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, "d1");
    }

    #[test]
    fn test_delete_draft_removes_row() {
        let conn = setup_db();
        insert_draft(
            &conn,
            &make_draft("d1", "acc1", "A", "2026-07-12T10:00:00Z"),
        )
        .unwrap();

        delete_draft(&conn, "d1").unwrap();

        assert!(get_drafts_by_account(&conn, "acc1").unwrap().is_empty());
    }

    #[test]
    fn test_delete_draft_missing_does_not_error() {
        let conn = setup_db();
        // 送信成功時の削除で「既に無い」を許容するため、エラーにしない
        delete_draft(&conn, "nonexistent").unwrap();
    }

    #[test]
    fn test_draft_in_reply_to_roundtrips() {
        let conn = setup_db();
        let mut draft = make_draft("d1", "acc1", "Re: 見積", "2026-07-12T10:00:00Z");
        draft.in_reply_to = Some("mail-123".into());
        insert_draft(&conn, &draft).unwrap();

        let drafts = get_drafts_by_account(&conn, "acc1").unwrap();
        assert_eq!(drafts[0].in_reply_to.as_deref(), Some("mail-123"));
    }
}
