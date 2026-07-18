use crate::db::mails::{row_to_mail, MAIL_COLUMNS_PREFIXED, MAIL_COLUMN_COUNT};
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
    project_id: Option<&str>,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let norm_query = crate::search_normalize::normalize_for_search(query.trim());
    if norm_query.is_empty() {
        return Ok(Vec::new());
    }

    if is_fts_eligible(&norm_query) {
        search_fts(conn, account_id, &norm_query, project_id, limit)
    } else {
        search_like(conn, account_id, &norm_query, project_id, limit)
    }
}

/// 案件サブツリースコープの共通 WHERE 述語。`?N` は呼び出し側の
/// バインド番号に合わせて渡す（同一パラメータを2箇所で使う）。
/// スコープ未指定（NULL）時は常に真になり、指定時のみサブツリー内の
/// project_id に絞り込む（未分類メールは対象外）。
fn scope_predicate(param_num: usize) -> String {
    format!(
        "(?{param_num} IS NULL OR mpa.project_id IN (
            WITH RECURSIVE scope(id) AS (
                SELECT id FROM projects WHERE id = ?{param_num}
                UNION ALL
                SELECT p2.id FROM projects p2 JOIN scope s ON p2.parent_id = s.id
            )
            SELECT id FROM scope
        ))"
    )
}

/// FTS5 trigram search for queries with 3+ characters (post-normalization).
/// `norm_query` must already be normalized via `search_normalize::normalize_for_search`.
fn search_fts(
    conn: &Connection,
    account_id: &str,
    norm_query: &str,
    project_id: Option<&str>,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let safe_query = sanitize_fts_query(norm_query);

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name
         FROM fts_mails fts
         JOIN mails m ON fts.mail_id = m.id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE fts_mails MATCH ?1
           AND m.account_id = ?2
           AND {}
         ORDER BY rank
         LIMIT ?4",
        *MAIL_COLUMNS_PREFIXED,
        scope_predicate(3)
    ))?;

    let results = stmt
        .query_map(params![safe_query, account_id, project_id, limit], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(MAIL_COLUMN_COUNT)?;
            let project_name: Option<String> = row.get(MAIL_COLUMN_COUNT + 1)?;
            Ok((mail, project_id, project_name))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results
        .into_iter()
        .map(|(mail, project_id, project_name)| {
            let snippet = snippet_for_mail(&mail, norm_query);
            SearchResult {
                mail,
                project_id,
                project_name,
                snippet,
            }
        })
        .collect())
}

/// 件名→本文の順でスニペットを試み、どちらにも無ければ件名をそのまま使う
/// （from/to だけにマッチした場合等のフォールバック）
fn snippet_for_mail(mail: &crate::models::mail::Mail, norm_query: &str) -> String {
    crate::search_snippet::make_snippet(&mail.subject, norm_query)
        .or_else(|| {
            mail.body_text
                .as_deref()
                .and_then(|body| crate::search_snippet::make_snippet(body, norm_query))
        })
        .unwrap_or_else(|| mail.subject.clone())
}

/// LIKE fallback for queries with < 3 characters (e.g. 2-char Japanese words).
/// Matches against the normalized fts_mails columns so query-side normalization
/// (hiragana/katakana folding, fullwidth/halfwidth, casing) applies here too.
/// `norm_query` must already be normalized via `search_normalize::normalize_for_search`.
fn search_like(
    conn: &Connection,
    account_id: &str,
    norm_query: &str,
    project_id: Option<&str>,
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let like_pattern = format!("%{}%", escape_like(norm_query));

    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name
         FROM fts_mails fts
         JOIN mails m ON fts.mail_id = m.id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE m.account_id = ?1
           AND (fts.subject LIKE ?2 ESCAPE '\\' OR fts.body_text LIKE ?2 ESCAPE '\\' OR fts.from_addr LIKE ?2 ESCAPE '\\' OR fts.to_addr LIKE ?2 ESCAPE '\\')
           AND {}
         ORDER BY m.date DESC
         LIMIT ?4",
        *MAIL_COLUMNS_PREFIXED,
        scope_predicate(3)
    ))?;

    let results = stmt
        .query_map(
            params![account_id, like_pattern, project_id, limit],
            |row| {
                let mail = row_to_mail(row)?;
                let project_id: Option<String> = row.get(MAIL_COLUMN_COUNT)?;
                let project_name: Option<String> = row.get(MAIL_COLUMN_COUNT + 1)?;
                Ok((mail, project_id, project_name))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results
        .into_iter()
        .map(|(mail, project_id, project_name)| {
            let snippet = snippet_for_mail(&mail, norm_query);
            SearchResult {
                mail,
                project_id,
                project_name,
                snippet,
            }
        })
        .collect())
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
        assert_eq!(
            sanitize_fts_query("user@example.com"),
            "\"user@example.com\""
        );
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

        // 4 chars — uses FTS trigram。索引には search_normalize::normalize_for_search
        // 適用後のテキストが入る（ひらがな→カタカナ折り畳み）。クエリ側も同じ正規化を
        // 適用してから照合するため、生ひらがなのクエリでカタカナ索引にヒットする。
        let results = search_mails(&conn, "acc1", "見積もり", None, 50).unwrap();
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
        let results = search_mails(&conn, "acc1", "予算", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_by_from_addr() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        m1.from_addr = "tanaka@example.com".into();
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "tanaka", None, 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "zzzznonexistent", None, 50).unwrap();
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

        let mut m1 = make_mail(
            "m1",
            "<msg1@ex.com>",
            "SharedKeyword",
            "2026-04-13T10:00:00",
        );
        m1.account_id = "acc1".into();
        let mut m2 = make_mail(
            "m2",
            "<msg2@ex.com>",
            "SharedKeyword",
            "2026-04-13T11:00:00",
        );
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();

        let results = search_mails(&conn, "acc1", "SharedKeyword", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_search_includes_project_info() {
        let conn = setup_db();
        let m1 = make_mail(
            "m1",
            "<msg1@ex.com>",
            "AlphaMail subject",
            "2026-04-13T10:00:00",
        );
        mails::insert_mail(&conn, &m1).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project Alpha".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();

        let results = search_mails(&conn, "acc1", "AlphaMail", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project_id, Some(proj.id));
        assert_eq!(results[0].project_name, Some("Project Alpha".into()));
    }

    #[test]
    fn test_search_unclassified_mail_has_no_project() {
        let conn = setup_db();
        let m1 = make_mail(
            "m1",
            "<msg1@ex.com>",
            "OrphanMail text",
            "2026-04-13T10:00:00",
        );
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "OrphanMail", None, 50).unwrap();
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

        let results = search_mails(&conn, "acc1", "CommonKeyword", None, 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_snippet_not_empty_fts() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Report", "2026-04-13T10:00:00");
        m1.body_text = Some("The quarterly revenue report shows growth in Q1".into());
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "revenue", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].snippet.is_empty());
    }

    #[test]
    fn test_search_empty_query_returns_empty() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "", None, 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_with_special_chars_no_error() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "foo-bar baz", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        // These should NOT cause FTS5 syntax errors
        let results = search_mails(&conn, "acc1", "foo-bar", None, 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_with_fts_operators_safely_handled() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello world", "2026-04-13T10:00:00");
        mails::insert_mail(&conn, &m1).unwrap();

        // FTS5 operators like AND, OR, NOT should be treated as literals
        let results = search_mails(&conn, "acc1", "AND OR NOT", None, 50).unwrap();
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
        let results = search_mails(&conn, "acc1", "予算", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_search_2char_like_snippet() {
        let conn = setup_db();
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "件名", "2026-04-13T10:00:00");
        m1.body_text = Some("本文の内容".into());
        mails::insert_mail(&conn, &m1).unwrap();

        let results = search_mails(&conn, "acc1", "件名", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        // LIKE フォールバック経路でも件名マッチが <b> ハイライトされる
        assert_eq!(results[0].snippet, "<b>件名</b>");
    }

    // --- normalization integration tests (Phase 1) ---

    #[test]
    fn test_search_halfwidth_query_matches_fullwidth_subject() {
        let conn = setup_db();
        let m = make_mail(
            "m1",
            "<m1@ex.com>",
            "ＳＡＴＯ商事お見積り",
            "2026-07-17T10:00:00",
        );
        mails::insert_mail(&conn, &m).unwrap();

        let results = search_mails(&conn, "acc1", "sato", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.id, "m1");
    }

    #[test]
    fn test_search_hiragana_query_matches_katakana_text() {
        let conn = setup_db();
        let mut m = make_mail("m1", "<m1@ex.com>", "端末の件", "2026-07-17T10:00:00");
        m.body_text = Some("サトー様のプリンターについて".into());
        mails::insert_mail(&conn, &m).unwrap();

        let results = search_mails(&conn, "acc1", "さとー", None, 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_2char_normalized_like_fallback() {
        let conn = setup_db();
        let mut m = make_mail("m1", "<m1@ex.com>", "件名", "2026-07-17T10:00:00");
        m.body_text = Some("ｻﾄｰの予算".into());
        mails::insert_mail(&conn, &m).unwrap();

        // 2 文字 → LIKE フォールバック側でも正規化照合される（ｻﾄ→サト）
        let results = search_mails(&conn, "acc1", "さと", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        // スニペットは本文の原文表記（半角カナのまま）でハイライトされる
        assert_eq!(results[0].snippet, "<b>ｻﾄ</b>ｰの予算");
    }

    #[test]
    fn test_search_snippet_shows_original_text() {
        let conn = setup_db();
        let mut m = make_mail("m1", "<m1@ex.com>", "Report", "2026-07-17T10:00:00");
        m.body_text = Some("ＳＡＴＯ商事より見積書が届きました".into());
        mails::insert_mail(&conn, &m).unwrap();

        let results = search_mails(&conn, "acc1", "sato", None, 50).unwrap();
        assert_eq!(results.len(), 1);
        // スニペットは正規化テキストでなく原文（全角のまま）で返る
        assert!(results[0].snippet.contains("<b>ＳＡＴＯ</b>"));
    }

    // --- project scope tests ---

    #[test]
    fn test_search_scoped_to_subtree() {
        let conn = setup_db();
        crate::db::projects::insert_project_with_id(
            &conn,
            "root",
            "acc1",
            "ツアー",
            None,
            None,
            None,
        )
        .unwrap();
        crate::db::projects::insert_project_with_id(
            &conn,
            "leaf",
            "acc1",
            "音響",
            None,
            None,
            Some("root"),
        )
        .unwrap();
        crate::db::projects::insert_project_with_id(
            &conn, "other", "acc1", "別件", None, None, None,
        )
        .unwrap();

        for (mid, pid, subj) in [
            ("m1", Some("leaf"), "スピーカー設営の件"),
            ("m2", Some("other"), "スピーカー購入の件"),
            ("m3", None, "スピーカー無関係未分類"),
        ] {
            let mut m = crate::test_helpers::make_mail(
                mid,
                &format!("<{mid}@ex>"),
                subj,
                "2026-07-18T10:00:00",
            );
            m.body_text = Some("スピーカー".into());
            // insert_mail が内部で index_mail を呼ぶため（v17でトリガー同期廃止）、
            // ここでの明示的な index_mail 呼び出しは不要（二重登録になる）
            crate::db::mails::insert_mail(&conn, &m).unwrap();
            if let Some(pid) = pid {
                crate::db::assignments::assign_mail(&conn, mid, pid, "user", None).unwrap();
            }
        }

        // スコープなし: 3件
        let all = search_mails(&conn, "acc1", "スピーカー", None, 50).unwrap();
        assert_eq!(all.len(), 3);
        // root スコープ: サブツリー内の m1 のみ（未分類 m3 は含まれない）
        let scoped = search_mails(&conn, "acc1", "スピーカー", Some("root"), 50).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].mail.id, "m1");
    }

    #[test]
    fn test_search_scoped_to_subtree_2char_like() {
        let conn = setup_db();
        crate::db::projects::insert_project_with_id(
            &conn,
            "root",
            "acc1",
            "ツアー",
            None,
            None,
            None,
        )
        .unwrap();
        crate::db::projects::insert_project_with_id(
            &conn,
            "leaf",
            "acc1",
            "音響",
            None,
            None,
            Some("root"),
        )
        .unwrap();
        crate::db::projects::insert_project_with_id(
            &conn, "other", "acc1", "別件", None, None, None,
        )
        .unwrap();

        for (mid, pid) in [("m1", Some("leaf")), ("m2", Some("other")), ("m3", None)] {
            let mut m = crate::test_helpers::make_mail(
                mid,
                &format!("<{mid}@ex>"),
                "件名",
                "2026-07-18T10:00:00",
            );
            m.body_text = Some("予算の件".into());
            crate::db::mails::insert_mail(&conn, &m).unwrap();
            if let Some(pid) = pid {
                crate::db::assignments::assign_mail(&conn, mid, pid, "user", None).unwrap();
            }
        }

        // 2 chars → LIKE フォールバック経路でもスコープが効く
        let scoped = search_mails(&conn, "acc1", "予算", Some("root"), 50).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].mail.id, "m1");
    }
}
