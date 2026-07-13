use crate::error::AppError;
use crate::models::attachment::Attachment;
use rusqlite::{params, Connection};
use uuid::Uuid;

pub fn insert_attachment(
    conn: &Connection,
    mail_id: &str,
    filename: &str,
    mime_type: &str,
    size: i64,
    file_path: &str,
    content_id: Option<&str>,
) -> Result<Attachment, AppError> {
    let attachment = Attachment {
        id: Uuid::new_v4().to_string(),
        mail_id: mail_id.to_string(),
        filename: filename.to_string(),
        mime_type: mime_type.to_string(),
        size: Some(size),
        file_path: Some(file_path.to_string()),
        content_id: content_id.map(|s| s.to_string()),
    };
    conn.execute(
        "INSERT INTO attachments (id, mail_id, filename, mime_type, size, file_path, content_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            attachment.id,
            attachment.mail_id,
            attachment.filename,
            attachment.mime_type,
            attachment.size,
            attachment.file_path,
            attachment.content_id,
        ],
    )?;
    Ok(attachment)
}

fn row_to_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        mail_id: row.get(1)?,
        filename: row.get(2)?,
        mime_type: row.get(3)?,
        size: row.get(4)?,
        file_path: row.get(5)?,
        content_id: row.get(6)?,
    })
}

pub fn get_by_mail_id(conn: &Connection, mail_id: &str) -> Result<Vec<Attachment>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, mail_id, filename, mime_type, size, file_path, content_id
         FROM attachments WHERE mail_id = ?1 ORDER BY filename",
    )?;
    let attachments = stmt
        .query_map(params![mail_id], row_to_attachment)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(attachments)
}

pub fn get_by_id(conn: &Connection, id: &str) -> Result<Attachment, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, mail_id, filename, mime_type, size, file_path, content_id
         FROM attachments WHERE id = ?1",
    )?;
    stmt.query_row(params![id], row_to_attachment)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::AttachmentNotFound(id.to_string()),
            other => AppError::Database(other),
        })
}

/// キャッシュ再構築時に mail_id の添付レコードを全置換する前提の削除
pub fn delete_by_mail_id(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM attachments WHERE mail_id = ?1",
        params![mail_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{insert_test_mail, setup_db};

    #[test]
    fn test_insert_and_get_by_mail_id() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");

        let att = insert_attachment(
            &conn,
            "m1",
            "report.pdf",
            "application/pdf",
            1024,
            "/cache/m1/report.pdf",
            None,
        )
        .unwrap();
        assert_eq!(att.mail_id, "m1");
        assert_eq!(att.size, Some(1024));

        let list = get_by_mail_id(&conn, "m1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].filename, "report.pdf");
        assert_eq!(list[0].mime_type, "application/pdf");
        assert_eq!(list[0].file_path.as_deref(), Some("/cache/m1/report.pdf"));
        assert!(list[0].content_id.is_none());
    }

    #[test]
    fn test_get_by_mail_id_empty() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "No attachment");
        assert!(get_by_mail_id(&conn, "m1").unwrap().is_empty());
    }

    #[test]
    fn test_get_by_mail_id_orders_by_filename() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Two attachments");
        insert_attachment(&conn, "m1", "b.txt", "text/plain", 1, "/c/b.txt", None).unwrap();
        insert_attachment(&conn, "m1", "a.txt", "text/plain", 1, "/c/a.txt", None).unwrap();

        let list = get_by_mail_id(&conn, "m1").unwrap();
        assert_eq!(list[0].filename, "a.txt");
        assert_eq!(list[1].filename, "b.txt");
    }

    #[test]
    fn test_get_by_id() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        let att =
            insert_attachment(&conn, "m1", "a.png", "image/png", 5, "/c/a.png", None).unwrap();

        let found = get_by_id(&conn, &att.id).unwrap();
        assert_eq!(found.filename, "a.png");
    }

    #[test]
    fn test_get_by_id_not_found() {
        let conn = setup_db();
        let err = get_by_id(&conn, "nope").unwrap_err();
        assert!(matches!(err, AppError::AttachmentNotFound(_)));
    }

    #[test]
    fn test_delete_by_mail_id() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        insert_attachment(&conn, "m1", "a.txt", "text/plain", 1, "/c/a.txt", None).unwrap();
        insert_attachment(&conn, "m1", "b.txt", "text/plain", 1, "/c/b.txt", None).unwrap();

        delete_by_mail_id(&conn, "m1").unwrap();
        assert!(get_by_mail_id(&conn, "m1").unwrap().is_empty());
    }

    #[test]
    fn test_insert_attachment_with_content_id() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Inline image");

        let att = insert_attachment(
            &conn,
            "m1",
            "logo.png",
            "image/png",
            100,
            "/cache/m1/logo.png",
            Some("logo123@example.com"),
        )
        .unwrap();
        assert_eq!(att.content_id.as_deref(), Some("logo123@example.com"));

        let found = get_by_id(&conn, &att.id).unwrap();
        assert_eq!(found.content_id.as_deref(), Some("logo123@example.com"));
    }
}
