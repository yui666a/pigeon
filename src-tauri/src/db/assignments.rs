use crate::error::AppError;
use crate::models::mail::Mail;
use rusqlite::{params, Connection};

/// INSERT OR REPLACE a mail-to-project assignment.
pub fn assign_mail(
    conn: &Connection,
    mail_id: &str,
    project_id: &str,
    assigned_by: &str,
    confidence: Option<f64>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO mail_project_assignments
         (mail_id, project_id, assigned_by, confidence)
         VALUES (?1, ?2, ?3, ?4)",
        params![mail_id, project_id, assigned_by, confidence],
    )?;
    Ok(())
}

/// Approve (or correct) a classification.
/// If `project_id` differs from the current assignment, records the old project
/// in `corrected_from` and updates `project_id`. Sets `assigned_by` to 'user'.
/// Returns `MailNotFound` if no assignment row exists for `mail_id`.
pub fn approve_classification(
    conn: &Connection,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    // Fetch current assignment
    let current_project: String = conn
        .query_row(
            "SELECT project_id FROM mail_project_assignments WHERE mail_id = ?1",
            params![mail_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::MailNotFound(mail_id.to_string()))?;

    if current_project == project_id {
        // Same project — just mark as user-approved
        conn.execute(
            "UPDATE mail_project_assignments
             SET assigned_by = 'user'
             WHERE mail_id = ?1",
            params![mail_id],
        )?;
    } else {
        // Different project — record correction
        conn.execute(
            "UPDATE mail_project_assignments
             SET project_id = ?1, assigned_by = 'user', corrected_from = ?2
             WHERE mail_id = ?3",
            params![project_id, current_project, mail_id],
        )?;
    }
    Ok(())
}

/// Delete the assignment for a mail (reject classification).
/// Returns `MailNotFound` if no assignment row exists for `mail_id`.
pub fn reject_classification(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "DELETE FROM mail_project_assignments WHERE mail_id = ?1",
        params![mail_id],
    )?;
    if affected == 0 {
        return Err(AppError::MailNotFound(mail_id.to_string()));
    }
    Ok(())
}

/// Get mails that have no project assignment for a given account.
pub fn get_unclassified_mails(
    conn: &Connection,
    account_id: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.account_id, m.folder, m.message_id, m.in_reply_to, m.\"references\",
                m.from_addr, m.to_addr, m.cc_addr, m.subject, m.body_text, m.body_html,
                m.date, m.has_attachments, m.raw_size, m.uid, m.flags, m.fetched_at
         FROM mails m
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.mail_id IS NULL AND m.account_id = ?1
         ORDER BY m.date DESC",
    )?;
    let mails = stmt
        .query_map(params![account_id], row_to_mail)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(mails)
}

/// Get mails assigned to a specific project.
pub fn get_mails_by_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.account_id, m.folder, m.message_id, m.in_reply_to, m.\"references\",
                m.from_addr, m.to_addr, m.cc_addr, m.subject, m.body_text, m.body_html,
                m.date, m.has_attachments, m.raw_size, m.uid, m.flags, m.fetched_at
         FROM mails m
         JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.project_id = ?1
         ORDER BY m.date DESC",
    )?;
    let mails = stmt
        .query_map(params![project_id], row_to_mail)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(mails)
}

/// Get recent mail subjects for a project (used as LLM context for classification).
pub fn get_recent_subjects(
    conn: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.subject
         FROM mails m
         JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.project_id = ?1
         ORDER BY m.date DESC
         LIMIT ?2",
    )?;
    let subjects = stmt
        .query_map(params![project_id, limit], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(subjects)
}

/// Get assignment info for a mail: (project_id, assigned_by, confidence).
pub fn get_assignment_info(
    conn: &Connection,
    mail_id: &str,
) -> Result<Option<(String, String, Option<f64>)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT project_id, assigned_by, confidence
         FROM mail_project_assignments
         WHERE mail_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![mail_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    match rows.next() {
        Some(Ok(info)) => Ok(Some(info)),
        Some(Err(e)) => Err(AppError::Database(e)),
        None => Ok(None),
    }
}

fn row_to_mail(row: &rusqlite::Row<'_>) -> rusqlite::Result<Mail> {
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
        fetched_at: row.get(17)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::accounts;
    use crate::db::mails;
    use crate::db::migrations::run_migrations;
    use crate::db::projects;
    use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest};
    use crate::models::mail::Mail;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn create_account(conn: &Connection, id: &str) {
        let req = CreateAccountRequest {
            name: "Test".into(),
            email: format!("{}@example.com", id),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Plain,
            provider: AccountProvider::Other,
            password: None,
        };
        accounts::insert_account_with_id(conn, id, &req).unwrap();
    }

    /// Creates a project with a specific ID.
    fn create_project(conn: &Connection, id: &str, account_id: &str, name: &str) {
        projects::insert_project_with_id(conn, id, account_id, name, None, None).unwrap();
    }

    fn make_mail(id: &str, account_id: &str, subject: &str, date: &str) -> Mail {
        Mail {
            id: id.into(),
            account_id: account_id.into(),
            folder: "INBOX".into(),
            message_id: format!("<{}@example.com>", id),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "me@example.com".into(),
            cc_addr: None,
            subject: subject.into(),
            body_text: Some("body".into()),
            body_html: None,
            date: date.into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        }
    }

    fn insert_mail(conn: &Connection, mail: &Mail) {
        mails::insert_mail(conn, mail).unwrap();
    }

    #[test]
    fn test_assign_and_get_by_project() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject A", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "acc1", "Subject B", "2026-04-13T11:00:00");
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);

        assign_mail(&conn, "m1", "proj1", "ai", Some(0.92)).unwrap();
        assign_mail(&conn, "m2", "proj1", "ai", Some(0.85)).unwrap();

        let result = get_mails_by_project(&conn, "proj1").unwrap();
        assert_eq!(result.len(), 2);
        // Ordered by date DESC
        assert_eq!(result[0].id, "m2");
        assert_eq!(result[1].id, "m1");

        // Verify assignment info
        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj1");
        assert_eq!(info.1, "ai");
        assert!((info.2.unwrap() - 0.92).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unclassified_mails() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Classified", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "acc1", "Unclassified", "2026-04-13T11:00:00");
        let m3 = make_mail("m3", "acc1", "Also Unclassified", "2026-04-13T12:00:00");
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        insert_mail(&conn, &m3);

        assign_mail(&conn, "m1", "proj1", "ai", Some(0.9)).unwrap();

        let unclassified = get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 2);
        // Ordered by date DESC
        assert_eq!(unclassified[0].id, "m3");
        assert_eq!(unclassified[1].id, "m2");

        // Different account should return empty
        create_account(&conn, "acc2");
        let unclassified_acc2 = get_unclassified_mails(&conn, "acc2").unwrap();
        assert!(unclassified_acc2.is_empty());
    }

    #[test]
    fn test_approve_same_project() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.88)).unwrap();

        // Approve with same project — just changes assigned_by to user
        approve_classification(&conn, "m1", "proj1").unwrap();

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj1");
        assert_eq!(info.1, "user");
    }

    #[test]
    fn test_approve_different_project() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.6)).unwrap();

        // Approve with different project — corrects the assignment
        approve_classification(&conn, "m1", "proj2").unwrap();

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj2");
        assert_eq!(info.1, "user");

        // Verify corrected_from is recorded
        let corrected_from: Option<String> = conn
            .query_row(
                "SELECT corrected_from FROM mail_project_assignments WHERE mail_id = 'm1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(corrected_from, Some("proj1".to_string()));
    }

    #[test]
    fn test_approve_nonexistent_returns_error() {
        let conn = setup_db();
        let result = approve_classification(&conn, "nonexistent", "proj1");
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_classification() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.5)).unwrap();

        reject_classification(&conn, "m1").unwrap();

        // Should now be unclassified
        let info = get_assignment_info(&conn, "m1").unwrap();
        assert!(info.is_none());

        let unclassified = get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
        assert_eq!(unclassified[0].id, "m1");
    }

    #[test]
    fn test_reject_nonexistent_returns_error() {
        let conn = setup_db();
        let result = reject_classification(&conn, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_recent_subjects() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "First", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "acc1", "Second", "2026-04-13T11:00:00");
        let m3 = make_mail("m3", "acc1", "Third", "2026-04-13T12:00:00");
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        insert_mail(&conn, &m3);

        assign_mail(&conn, "m1", "proj1", "ai", None).unwrap();
        assign_mail(&conn, "m2", "proj1", "ai", None).unwrap();
        assign_mail(&conn, "m3", "proj1", "ai", None).unwrap();

        // Limit 2 — should get the 2 most recent by date
        let subjects = get_recent_subjects(&conn, "proj1", 2).unwrap();
        assert_eq!(subjects.len(), 2);
        assert_eq!(subjects[0], "Third");
        assert_eq!(subjects[1], "Second");

        // Limit 10 — should get all 3
        let all_subjects = get_recent_subjects(&conn, "proj1", 10).unwrap();
        assert_eq!(all_subjects.len(), 3);
    }

    #[test]
    fn test_assign_mail_replaces_existing() {
        let conn = setup_db();
        create_account(&conn, "acc1");
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);

        assign_mail(&conn, "m1", "proj1", "ai", Some(0.8)).unwrap();
        // INSERT OR REPLACE should update the row
        assign_mail(&conn, "m1", "proj2", "user", Some(1.0)).unwrap();

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj2");
        assert_eq!(info.1, "user");
    }
}
