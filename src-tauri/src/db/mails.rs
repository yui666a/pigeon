use crate::error::AppError;
use crate::models::mail::{Mail, Thread};
use rusqlite::{params, Connection};
use std::collections::HashMap;

pub fn insert_mail(conn: &Connection, mail: &Mail) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO mails
         (id, account_id, folder, message_id, in_reply_to, \"references\",
          from_addr, to_addr, cc_addr, subject, body_text, body_html,
          date, has_attachments, raw_size, uid, flags, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
            mail.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn get_mails_by_account(
    conn: &Connection,
    account_id: &str,
    folder: &str,
) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, folder, message_id, in_reply_to, \"references\",
                from_addr, to_addr, cc_addr, subject, body_text, body_html,
                date, has_attachments, raw_size, uid, flags, fetched_at
         FROM mails WHERE account_id = ?1 AND folder = ?2 ORDER BY date DESC",
    )?;
    let mails = stmt
        .query_map(params![account_id, folder], |row| {
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
        })?
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
    use crate::db::migrations::run_migrations;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        ).unwrap();
        conn
    }

    fn make_mail(id: &str, message_id: &str, subject: &str, date: &str) -> Mail {
        Mail {
            id: id.into(),
            account_id: "acc1".into(),
            folder: "INBOX".into(),
            message_id: message_id.into(),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "me@example.com".into(),
            cc_addr: None,
            subject: subject.into(),
            body_text: Some("Hello".into()),
            body_html: None,
            date: date.into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        }
    }

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
    fn test_get_max_uid() {
        let conn = setup_db();
        assert_eq!(get_max_uid(&conn, "acc1", "INBOX").unwrap(), 0);
        let mut mail = make_mail("m1", "<msg1@example.com>", "Test", "2026-04-13T10:00:00");
        mail.uid = 42;
        insert_mail(&conn, &mail).unwrap();
        assert_eq!(get_max_uid(&conn, "acc1", "INBOX").unwrap(), 42);
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
        let mut m3 = make_mail("m3", "<msg3@ex.com>", "Re: Re: Topic", "2026-04-13T12:00:00");
        m3.in_reply_to = Some("<msg2@ex.com>".into());
        let mut m4 = make_mail("m4", "<msg4@ex.com>", "Re: Re: Re: Topic", "2026-04-13T13:00:00");
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
