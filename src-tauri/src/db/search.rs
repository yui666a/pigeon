use crate::db::mails::{row_to_mail, MAIL_COLUMNS_PREFIXED};
use crate::error::AppError;
use crate::models::mail::SearchResult;
use rusqlite::{params, Connection};

/// Sanitize user input for FTS5 trigram queries.
/// Wraps the input in double quotes to treat it as a literal substring match,
/// escaping any internal double quotes.
fn sanitize_fts_query(query: &str) -> String {
    let escaped = query.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

/// Check if a query is long enough for FTS5 trigram (>= 3 characters).
/// Trigram tokenizer creates 3-character tokens, so shorter queries
/// cannot match and must use LIKE fallback.
fn is_fts_eligible(query: &str) -> bool {
    query.chars().count() >= 3
}

/// Escape LIKE special characters (`%`, `_`, `\`) so user input is
/// treated as a literal substring. Uses `\` as the ESCAPE character.
fn escape_like(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len());
    for ch in query.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Search mails using FTS5 trigram for queries >= 3 chars,
/// or LIKE fallback for shorter queries (e.g. 2-char Japanese words).
/// `account_id` scopes the search to a single account.
/// Returns up to `limit` results.
pub fn search_mails(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if is_fts_eligible(trimmed) {
        search_fts(conn, account_id, trimmed, limit)
    } else {
        search_like(conn, account_id, trimmed, limit)
    }
}

/// FTS5 trigram search for queries with 3+ characters.
fn search_fts(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let safe_query = sanitize_fts_query(query);

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name,
                snippet(fts_mails, 1, '<b>', '</b>', '...', 32) AS snip
         FROM fts_mails fts
         JOIN mails m ON fts.mail_id = m.id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE fts_mails MATCH ?1
           AND m.account_id = ?2
         ORDER BY rank
         LIMIT ?3",
        MAIL_COLUMNS_PREFIXED
    ))?;

    let results = stmt
        .query_map(params![safe_query, account_id, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(18)?;
            let project_name: Option<String> = row.get(19)?;
            let snippet: String = row.get(20)?;
            Ok(SearchResult {
                mail,
                project_id,
                project_name,
                snippet,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// LIKE fallback for queries with < 3 characters (e.g. 2-char Japanese words).
/// No FTS5 ranking or snippet available, so we use subject as snippet.
fn search_like(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let like_pattern = format!("%{}%", escape_like(query));

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name
         FROM mails m
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE m.account_id = ?1
           AND (m.subject LIKE ?2 ESCAPE '\\' OR m.body_text LIKE ?2 ESCAPE '\\' OR m.from_addr LIKE ?2 ESCAPE '\\' OR m.to_addr LIKE ?2 ESCAPE '\\')
         ORDER BY m.date DESC
         LIMIT ?3",
        MAIL_COLUMNS_PREFIXED
    ))?;

    let results = stmt
        .query_map(params![account_id, like_pattern, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(18)?;
            let project_name: Option<String> = row.get(19)?;
            Ok(SearchResult {
                mail: mail.clone(),
                project_id,
                project_name,
                snippet: mail.subject,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{assignments, mails, projects};
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{make_mail, setup_db};

    // --- sanitize_fts_query tests ---

    #[test]
    fn test_sanitize_plain_text() {
        assert_eq!(sanitize_fts_query("hello"), "\"hello\"");
    }

    #[test]
    fn test_sanitize_with_special_chars() {
        assert_eq!(sanitize_fts_query("foo-bar"), "\"foo-bar\"");
        assert_eq!(sanitize_fts_query("user@example.com"), "\"user@example.com\"");
    }

    #[test]
    fn test_sanitize_with_double_quotes() {
        assert_eq!(sanitize_fts_query("say \"hello\""), "\"say \"\"hello\"\"\"");
    }

    #[test]
    fn test_sanitize_japanese() {
        assert_eq!(sanitize_fts_query("見積もり"), "\"見積もり\"");
    }

    // --- is_fts_eligible tests ---

    #[test]
    fn test_fts_eligible_3_chars() {
        assert!(is_fts_eligible("abc"));
        assert!(is_fts_eligible("見積も"));
    }

    #[test]
    fn test_fts_eligible_2_chars() {
        assert!(!is_fts_eligible("ab"));
        assert!(!is_fts_eligible("予算"));
    }

    #[test]
    fn test_fts_eligible_1_char() {
        assert!(!is_fts_eligible("a"));
        assert!(!is_fts_eligible("予"));
    }

    // --- escape_like tests ---

    #[test]
    fn test_escape_like_plain() {
        assert_eq!(escape_like("hello"), "hello");
    }

    #[test]
    fn test_escape_like_percent() {
        assert_eq!(escape_like("100%"), "100\\%");
    }

    #[test]
    fn test_escape_like_underscore() {
        assert_eq!(escape_like("a_b"), "a\\_b");
    }

    #[test]
    fn test_escape_like_backslash() {
        assert_eq!(escape_like("a\\b"), "a\\\\b");
    }

    // --- search_mails tests ---

    #[test]
    fn test_search_by_subject_3plus_chars() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "見積もりの件", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "議事録の共有", "2026-04-13T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        // 4 chars — uses FTS trigram
        let results = search_mails(&conn, "acc1", "見積もり", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_by_subject_2char_japanese() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Subject", "2026-04-13T10:00:00");
        m1.body_text = Some("プロジェクトの予算について相談があります".into());
        mails::insert_mail(&conn, &m1).unwrap();

        // 2 chars — uses LIKE fallback
        let results = search_mails(&conn, "acc1", "予算", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_by_from_addr() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        m1.from_addr = "tanaka@example.com".into();
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "tanaka", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "zzzznonexistent", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        let mut m1 = make_mail("m1", "<msg1@ex.com>", "SharedKeyword", "2026-04-13T10:00:00");
        m1.account_id = "acc1".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "SharedKeyword", "2026-04-13T11:00:00");
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        let results = search_mails(&conn, "acc1", "SharedKeyword", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_search_includes_project_info() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "AlphaMail subject", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project Alpha".into(),
            description: None,
            color: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();

        let results = search_mails(&conn, "acc1", "AlphaMail", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project_id, Some(proj.id));
        assert_eq!(results[0].project_name, Some("Project Alpha".into()));
    }

    #[test]
    fn test_search_unclassified_mail_has_no_project() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "OrphanMail text", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "OrphanMail", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].project_id.is_none());
        assert!(results[0].project_name.is_none());
    }

    #[test]
    fn test_search_respects_limit() {
        let conn = setup_db();
        for i in 0..10 {
            let m = make_mail(
                &format!("m{}", i),
                &format!("<msg{}@ex.com>", i),
                &format!("CommonKeyword item{}", i),
                &format!("2026-04-13T1{}:00:00", i),
            );
            mails::insert_mail(&conn, &m).unwrap();
        }

        let results = search_mails(&conn, "acc1", "CommonKeyword", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_snippet_not_empty_fts() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Report", "2026-04-13T10:00:00");
        m1.body_text = Some("The quarterly revenue report shows growth in Q1".into());
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "revenue", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].snippet.is_empty());
    }

    #[test]
    fn test_search_empty_query_returns_empty() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_with_special_chars_no_error() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "foo-bar baz", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        // These should NOT cause FTS5 syntax errors
        let results = search_mails(&conn, "acc1", "foo-bar", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_with_fts_operators_safely_handled() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello world", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        // FTS5 operators like AND, OR, NOT should be treated as literals
        let results = search_mails(&conn, "acc1", "AND OR NOT", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_2char_like_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        let mut m1 = make_mail("m1", "<msg1@ex.com>", "予算の件", "2026-04-13T10:00:00");
        m1.account_id = "acc1".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "予算計画", "2026-04-13T11:00:00");
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        // 2 chars → LIKE fallback, should still be scoped to account
        let results = search_mails(&conn, "acc1", "予算", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_search_2char_like_snippet() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "件名", "2026-04-13T10:00:00");
        m1.body_text = Some("本文の内容".into());
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "件名", 50).unwrap();
        assert_eq!(results.len(), 1);
        // LIKE fallback snippet should use subject as fallback
        assert!(!results[0].snippet.is_empty());
    }
}
