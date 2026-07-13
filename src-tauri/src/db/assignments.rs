use crate::db::mails::{self, row_to_mail, MAIL_COLUMNS_PREFIXED};
use crate::error::AppError;
use crate::models::mail::Mail;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

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
    // assignment の更新と correction_log への記録を原子的に行う
    let tx = conn.unchecked_transaction()?;
    approve_classification_in_tx(&tx, mail_id, project_id)?;
    tx.commit()?;
    Ok(())
}

/// `approve_classification` の本体。呼び出し側でトランザクション境界を張ること
/// （`move_mail_to_project` が自トランザクション内から再利用する）。
fn approve_classification_in_tx(
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
        // Record in correction_log for LLM feedback
        insert_correction(conn, mail_id, Some(&current_project), project_id)?;
    }
    Ok(())
}

/// Delete the assignment for a mail (reject classification).
/// Returns `MailNotFound` if no assignment row exists for `mail_id`.
///
/// 却下は「このメールをこの案件に入れない」というユーザーの明示的な意思表示のため、
/// スレッド追従の除外トゥームストーン（`follow_exclusions`）に記録する。これにより
/// `auto_follow_threads` がスレッド仲間の割り当てを根拠に黙って再割り当てするのを防ぐ。
/// ユーザーが後から手動で割り当て直す（`move_mail_to_project`）と除外は解除される。
pub fn reject_classification(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    // 割り当て解除と除外トゥームストーンの記録を原子的に行う
    let tx = conn.unchecked_transaction()?;
    let affected = tx.execute(
        "DELETE FROM mail_project_assignments WHERE mail_id = ?1",
        params![mail_id],
    )?;
    if affected == 0 {
        return Err(AppError::MailNotFound(mail_id.to_string()));
    }
    add_follow_exclusion(&tx, mail_id)?;
    tx.commit()?;
    Ok(())
}

/// スレッド追従の除外トゥームストーンにメールを記録する（冪等）。
pub fn add_follow_exclusion(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR IGNORE INTO follow_exclusions (mail_id) VALUES (?1)",
        params![mail_id],
    )?;
    Ok(())
}

/// スレッド追従の除外トゥームストーンからメールを取り除く（冪等）。
/// ユーザーが手動で割り当て直したときに「意思を変えた」として除外を解く。
pub fn remove_follow_exclusion(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM follow_exclusions WHERE mail_id = ?1",
        params![mail_id],
    )?;
    Ok(())
}

/// Get mails that have no project assignment for a given account.
/// 分類対象は受信メールのみ（INBOX）。自分の送信済み（Sent）や
/// アーカイブ済み（Archive）を未分類として扱わない
pub fn get_unclassified_mails(conn: &Connection, account_id: &str) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM mails m
             LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
             WHERE mpa.mail_id IS NULL AND m.account_id = ?1 AND m.folder = 'INBOX'
             ORDER BY m.date DESC",
        MAIL_COLUMNS_PREFIXED
    ))?;
    let mails = stmt
        .query_map(params![account_id], row_to_mail)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mails)
}

/// Get mails assigned to a specific project.
pub fn get_mails_by_project(conn: &Connection, project_id: &str) -> Result<Vec<Mail>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM mails m
             JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
             WHERE mpa.project_id = ?1
             ORDER BY m.date DESC",
        MAIL_COLUMNS_PREFIXED
    ))?;
    let mails = stmt
        .query_map(params![project_id], row_to_mail)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
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
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(subjects)
}

/// 案件に割り当て済みメールの送信者(from_addr)を頻度降順で返す。
/// 同数のときは from_addr 昇順で安定させる。分類プロンプトの手がかり用。
pub fn get_top_senders(
    conn: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.from_addr
         FROM mails m
         JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         WHERE mpa.project_id = ?1
         GROUP BY m.from_addr
         ORDER BY COUNT(*) DESC, m.from_addr ASC
         LIMIT ?2",
    )?;
    let senders = stmt
        .query_map(params![project_id, limit], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(senders)
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

/// Record a user correction in the correction_log table.
pub fn insert_correction(
    conn: &Connection,
    mail_id: &str,
    from_project: Option<&str>,
    to_project: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO correction_log (mail_id, from_project, to_project)
         VALUES (?1, ?2, ?3)",
        params![mail_id, from_project, to_project],
    )?;
    Ok(())
}

/// Get recent corrections for an account (used as few-shot examples in LLM prompts).
/// Returns the last `limit` corrections with mail subjects and project names.
pub fn get_recent_corrections(
    conn: &Connection,
    account_id: &str,
    limit: u32,
) -> Result<Vec<crate::models::classifier::CorrectionEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.subject,
                pf.name AS from_project_name,
                pt.name AS to_project_name
         FROM correction_log cl
         JOIN mails m ON cl.mail_id = m.id
         JOIN projects pt ON cl.to_project = pt.id
         LEFT JOIN projects pf ON cl.from_project = pf.id
         WHERE m.account_id = ?1
         ORDER BY cl.corrected_at DESC, cl.id DESC
         LIMIT ?2",
    )?;
    let corrections = stmt
        .query_map(params![account_id, limit], |row| {
            Ok(crate::models::classifier::CorrectionEntry {
                mail_subject: row.get(0)?,
                from_project: row.get(1)?,
                to_project: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(corrections)
}

/// Move a mail to a project. Handles both classified and unclassified mails.
/// If the mail was already assigned, updates the assignment and logs the correction.
/// If unclassified, creates a new assignment and logs the correction.
pub fn move_mail_to_project(
    conn: &Connection,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    // assignment・correction_log・除外解除の複数書き込みを原子的に行う
    let tx = conn.unchecked_transaction()?;
    let current = get_assignment_info(&tx, mail_id)?;
    match current {
        Some((current_project_id, _, _)) => {
            // Already assigned — reuse approve logic which handles correction_log
            if current_project_id != project_id {
                approve_classification_in_tx(&tx, mail_id, project_id)?;
            }
        }
        None => {
            // Unclassified — create new assignment and log
            assign_mail(&tx, mail_id, project_id, "user", Some(1.0))?;
            insert_correction(&tx, mail_id, None, project_id)?;
        }
    }
    // ユーザーが明示的に案件へ割り当てた＝過去の却下の意思を撤回したとみなし、
    // スレッド追従の除外を解除する（以後はこのメールも追従対象に戻る）
    remove_follow_exclusion(&tx, mail_id)?;
    tx.commit()?;
    Ok(())
}

/// スレッド追従の自動分類: 未分類メールのスレッド仲間が単一の案件に割り当て済みなら、
/// その案件へ自動的に追従割り当てする。スレッド仲間が複数の異なる案件に割り当てられている
/// 場合は曖昧なので追従しない。
///
/// 判定は `mails::build_threads` と同じロジック（In-Reply-To/References + 件名フォールバック）
/// をアカウント全フォルダのメールに対して適用する。設計:
/// docs/superpowers/specs/2026-07-13-thread-follow-classify-design.md
///
/// `assigned_by` は "ai"、`confidence` は None を使う（AIの意味的分類ではなく構造的な
/// 推論のため、確信度スコアを持たないことで区別する）。ユーザーの訂正判断を経由しない
/// 機械的な追従のため `correction_log` には記録しない（誤った学習信号になるのを避ける）。
///
/// 戻り値は追従割り当てしたメール数。
pub fn auto_follow_threads(conn: &Connection, account_id: &str) -> Result<usize, AppError> {
    // 読み出しは3クエリに固定する: 軽量メタ（本文カラムを読まない）・割り当てマップ・除外集合。
    // 以前は本文込み全メールのロード + メール毎の get_assignment_info 発行（N+1）だった
    let metas = mails::get_thread_metas_by_account(conn, account_id)?;
    let thread_mail_ids = mails::group_mail_ids_into_threads(&metas);
    let assigned = get_assignment_map(conn, account_id)?;
    // ユーザーが却下したメールは追従の対象外（トゥームストーン）
    let excluded = get_follow_exclusions(conn, account_id)?;

    // 追従の書き込みは1トランザクションに束ねる。本処理は一覧取得のたびに再実行される
    // 冪等な処理のため部分成功でも次回リカバリはされるが、単一コミットにすることで
    // INSERT 毎の autocommit を避けられ、途中失敗時に「スレッドの一部だけ追従された」
    // 中途半端な状態を残さない
    let tx = conn.unchecked_transaction()?;
    let mut followed = 0;
    for mail_ids in &thread_mail_ids {
        let assigned_projects: HashSet<&String> =
            mail_ids.iter().filter_map(|id| assigned.get(id)).collect();

        // ちょうど1件の案件に統一されているスレッドのみ追従する。
        // 0件（誰も割り当てられていない）や複数件（曖昧）は対象外
        if assigned_projects.len() != 1 {
            continue;
        }
        let target_project = match assigned_projects.into_iter().next() {
            Some(p) => p,
            None => continue,
        };

        for mail_id in mail_ids {
            // 却下済み（除外トゥームストーンあり）のメールは黙って再割り当てしない。
            // 割り当て済みのメールもスキップ（スレッドは互いに素なので先読みマップで判定できる）
            if excluded.contains(mail_id) || assigned.contains_key(mail_id) {
                continue;
            }
            assign_mail(&tx, mail_id, target_project, "ai", None)?;
            followed += 1;
        }
    }
    tx.commit()?;
    Ok(followed)
}

/// アカウント配下メールの割り当てを一括で読み出す（mail_id → project_id）。
/// `auto_follow_threads` がメール毎に `get_assignment_info` を発行する N+1 を
/// 避けるための先読み用。
fn get_assignment_map(
    conn: &Connection,
    account_id: &str,
) -> Result<HashMap<String, String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT mpa.mail_id, mpa.project_id
         FROM mail_project_assignments mpa
         JOIN mails m ON m.id = mpa.mail_id
         WHERE m.account_id = ?1",
    )?;
    let map = stmt
        .query_map(params![account_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(map)
}

/// アカウントに属するメールのうち、スレッド追従から除外されているメールIDの集合を返す。
fn get_follow_exclusions(conn: &Connection, account_id: &str) -> Result<HashSet<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT fe.mail_id
         FROM follow_exclusions fe
         JOIN mails m ON m.id = fe.mail_id
         WHERE m.account_id = ?1",
    )?;
    let excluded = stmt
        .query_map(params![account_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(excluded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::accounts;
    use crate::db::mails;
    use crate::db::projects;
    use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest};
    use crate::models::mail::Mail;
    // 共有ヘルパの setup_db は FK 有効化・マイグレーション適用済みで、
    // テストアカウント acc1 を作成済みの接続を返す
    use crate::test_helpers::setup_db;
    use rusqlite::Connection;

    /// acc1 以外の追加アカウントを作成する（acc1 は setup_db が作成済み）。
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
            // (account_id, folder, uid) は UNIQUE (migrate_v6) のため id から導出
            uid: id
                .bytes()
                .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(u32::from(b))),
            flags: None,
            is_read: false,
            is_flagged: false,
            fetched_at: "2026-04-13T00:00:00".into(),
            uid_confirmed: true,
        }
    }

    fn insert_mail(conn: &Connection, mail: &Mail) {
        mails::insert_mail(conn, mail).unwrap();
    }

    #[test]
    fn test_assign_and_get_by_project() {
        let conn = setup_db();
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

    // 指定送信者のメールを作って project に割り当てる（get_top_senders テスト用）。
    fn assign_mail_from(conn: &Connection, id: &str, project_id: &str, from: &str) {
        let mut mail = make_mail(id, "acc1", "subj", "2026-04-13T10:00:00");
        mail.from_addr = from.to_string();
        insert_mail(conn, &mail);
        assign_mail(conn, id, project_id, "ai", Some(0.9)).unwrap();
    }

    #[test]
    fn test_get_top_senders_orders_by_frequency() {
        let conn = setup_db();
        create_project(&conn, "p1", "acc1", "P1");
        assign_mail_from(&conn, "m1", "p1", "a@x.com");
        assign_mail_from(&conn, "m2", "p1", "a@x.com");
        assign_mail_from(&conn, "m3", "p1", "a@x.com");
        assign_mail_from(&conn, "m4", "p1", "b@x.com");
        assign_mail_from(&conn, "m5", "p1", "b@x.com");
        assign_mail_from(&conn, "m6", "p1", "c@x.com");

        let senders = get_top_senders(&conn, "p1", 5).unwrap();
        assert_eq!(senders, vec!["a@x.com", "b@x.com", "c@x.com"]);
    }

    #[test]
    fn test_get_top_senders_respects_limit() {
        let conn = setup_db();
        create_project(&conn, "p1", "acc1", "P1");
        assign_mail_from(&conn, "m1", "p1", "a@x.com");
        assign_mail_from(&conn, "m2", "p1", "b@x.com");
        assign_mail_from(&conn, "m3", "p1", "c@x.com");
        let senders = get_top_senders(&conn, "p1", 2).unwrap();
        assert_eq!(senders.len(), 2);
    }

    #[test]
    fn test_get_top_senders_ties_broken_by_addr_asc() {
        let conn = setup_db();
        create_project(&conn, "p1", "acc1", "P1");
        // 全員1通ずつ（同数）→ from_addr 昇順で安定
        assign_mail_from(&conn, "m1", "p1", "zoe@x.com");
        assign_mail_from(&conn, "m2", "p1", "amy@x.com");
        assign_mail_from(&conn, "m3", "p1", "mia@x.com");
        let senders = get_top_senders(&conn, "p1", 5).unwrap();
        assert_eq!(senders, vec!["amy@x.com", "mia@x.com", "zoe@x.com"]);
    }

    #[test]
    fn test_get_top_senders_empty_for_unassigned_project() {
        let conn = setup_db();
        let senders = get_top_senders(&conn, "no-such-project", 5).unwrap();
        assert!(senders.is_empty());
    }

    #[test]
    fn test_assign_mail_replaces_existing() {
        let conn = setup_db();
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

    #[test]
    fn test_insert_and_get_corrections() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        let m1 = make_mail("m1", "acc1", "Mail Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);

        insert_correction(&conn, "m1", Some("proj1"), "proj2").unwrap();

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].mail_subject, "Mail Subject");
        assert_eq!(
            corrections[0].from_project,
            Some("Project Alpha".to_string())
        );
        assert_eq!(corrections[0].to_project, "Project Beta");
    }

    #[test]
    fn test_correction_from_unclassified() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);

        insert_correction(&conn, "m1", None, "proj1").unwrap();

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert_eq!(corrections.len(), 1);
        assert!(corrections[0].from_project.is_none());
        assert_eq!(corrections[0].to_project, "Project Alpha");
    }

    #[test]
    fn test_corrections_limited_and_ordered() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        for i in 0..5 {
            let m = make_mail(
                &format!("m{}", i),
                "acc1",
                &format!("Subject {}", i),
                &format!("2026-04-13T1{}:00:00", i),
            );
            insert_mail(&conn, &m);
            insert_correction(&conn, &format!("m{}", i), Some("proj1"), "proj2").unwrap();
        }

        let corrections = get_recent_corrections(&conn, "acc1", 3).unwrap();
        assert_eq!(corrections.len(), 3);
        // Most recent first
        assert_eq!(corrections[0].mail_subject, "Subject 4");
    }

    #[test]
    fn test_approve_classification_writes_correction_log() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.8)).unwrap();

        approve_classification(&conn, "m1", "proj2").unwrap();

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert_eq!(corrections.len(), 1);
        assert_eq!(
            corrections[0].from_project,
            Some("Project Alpha".to_string())
        );
        assert_eq!(corrections[0].to_project, "Project Beta");
    }

    #[test]
    fn test_approve_same_project_no_correction_log() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.8)).unwrap();

        approve_classification(&conn, "m1", "proj1").unwrap();

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert!(corrections.is_empty());
    }

    #[test]
    fn test_move_mail_from_unclassified() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);

        move_mail_to_project(&conn, "m1", "proj1").unwrap();

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj1");
        assert_eq!(info.1, "user");

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert_eq!(corrections.len(), 1);
        assert!(corrections[0].from_project.is_none());
    }

    #[test]
    fn test_move_mail_between_projects() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.9)).unwrap();

        move_mail_to_project(&conn, "m1", "proj2").unwrap();

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj2");

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert_eq!(corrections.len(), 1);
    }

    #[test]
    fn test_move_mail_to_same_project_noop() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.9)).unwrap();

        move_mail_to_project(&conn, "m1", "proj1").unwrap();

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert!(corrections.is_empty());
    }

    // --- トランザクション境界（B-2: assignment 更新と correction_log の原子性） ---

    /// 失敗注入: correction_log への INSERT を必ず失敗させるトリガーを張る
    fn inject_correction_log_failure(conn: &Connection) {
        conn.execute_batch(
            "CREATE TRIGGER fail_correction_log BEFORE INSERT ON correction_log
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    }

    #[test]
    fn test_approve_classification_rolls_back_assignment_on_correction_log_failure() {
        // assignment の UPDATE 成功後に correction_log の INSERT が失敗したら、
        // UPDATE ごとロールバックされること（訂正の記録なき割り当て変更を残さない）
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");
        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.6)).unwrap();

        inject_correction_log_failure(&conn);

        let result = approve_classification(&conn, "m1", "proj2");
        assert!(result.is_err(), "注入した失敗がエラーとして伝播する");

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj1", "assignment の更新がロールバックされる");
        assert_eq!(info.1, "ai", "assigned_by も元のまま");
        let corrected_from: Option<String> = conn
            .query_row(
                "SELECT corrected_from FROM mail_project_assignments WHERE mail_id = 'm1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(corrected_from.is_none(), "corrected_from も書き込まれない");
    }

    #[test]
    fn test_move_mail_to_project_rolls_back_new_assignment_on_correction_log_failure() {
        // 未分類メールへの新規割り当てでも、correction_log の失敗で
        // assignment の INSERT ごとロールバックされること
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);

        inject_correction_log_failure(&conn);

        let result = move_mail_to_project(&conn, "m1", "proj1");
        assert!(result.is_err(), "注入した失敗がエラーとして伝播する");

        let info = get_assignment_info(&conn, "m1").unwrap();
        assert!(info.is_none(), "新規 assignment がロールバックされ未分類のまま");
    }

    #[test]
    fn test_move_mail_between_projects_rolls_back_on_correction_log_failure() {
        // 割り当て済みメールの移動（approve_classification 経由）でも原子性が保たれること
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");
        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.9)).unwrap();

        inject_correction_log_failure(&conn);

        let result = move_mail_to_project(&conn, "m1", "proj2");
        assert!(result.is_err());

        let info = get_assignment_info(&conn, "m1").unwrap().unwrap();
        assert_eq!(info.0, "proj1", "移動がロールバックされる");
    }

    #[test]
    fn test_reject_classification_rolls_back_delete_on_exclusion_failure() {
        // 却下は DELETE（assignment）+ INSERT（follow_exclusions）の複数書き込み。
        // トゥームストーンの記録に失敗したら割り当て解除ごとロールバックされること
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        let m1 = make_mail("m1", "acc1", "Subject", "2026-04-13T10:00:00");
        insert_mail(&conn, &m1);
        assign_mail(&conn, "m1", "proj1", "ai", Some(0.5)).unwrap();

        conn.execute_batch(
            "CREATE TRIGGER fail_exclusion BEFORE INSERT ON follow_exclusions
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

        let result = reject_classification(&conn, "m1");
        assert!(result.is_err(), "注入した失敗がエラーとして伝播する");

        let info = get_assignment_info(&conn, "m1").unwrap();
        assert!(info.is_some(), "assignment の削除がロールバックされる");
    }

    // --- auto_follow_threads flow ---

    #[test]
    fn test_auto_follow_assigns_reply_to_threadmates_project() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);

        // m1 is already assigned; m2 (a reply) is unclassified
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        let followed = auto_follow_threads(&conn, "acc1").unwrap();
        assert_eq!(followed, 1);

        let info = get_assignment_info(&conn, "m2").unwrap().unwrap();
        assert_eq!(info.0, "proj1");
    }

    #[test]
    fn test_auto_follow_skips_thread_split_across_multiple_projects() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");
        create_project(&conn, "proj2", "acc1", "Project Beta");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        let mut m3 = make_mail("m3", "acc1", "Re: Test", "2026-04-13T12:00:00");
        m3.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        insert_mail(&conn, &m3);

        // Threadmates disagree on the project — ambiguous, m3 should stay unclassified
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();
        assign_mail(&conn, "m2", "proj2", "user", Some(1.0)).unwrap();

        let followed = auto_follow_threads(&conn, "acc1").unwrap();
        assert_eq!(followed, 0);

        let info = get_assignment_info(&conn, "m3").unwrap();
        assert!(info.is_none(), "曖昧なスレッドは未分類のまま");
    }

    #[test]
    fn test_auto_follow_noop_when_no_threadmate_assigned() {
        let conn = setup_db();

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);

        let followed = auto_follow_threads(&conn, "acc1").unwrap();
        assert_eq!(followed, 0);
        assert!(get_assignment_info(&conn, "m1").unwrap().is_none());
        assert!(get_assignment_info(&conn, "m2").unwrap().is_none());
    }

    #[test]
    fn test_auto_follow_does_not_affect_unrelated_threads() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        // Unrelated thread, unassigned throughout
        let m3 = make_mail("m3", "acc1", "Totally Unrelated", "2026-04-13T09:00:00");
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        insert_mail(&conn, &m3);

        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        let followed = auto_follow_threads(&conn, "acc1").unwrap();
        assert_eq!(followed, 1);
        assert!(
            get_assignment_info(&conn, "m3").unwrap().is_none(),
            "無関係なスレッドは影響を受けない"
        );
    }

    #[test]
    fn test_auto_follow_does_not_write_correction_log() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        auto_follow_threads(&conn, "acc1").unwrap();

        let corrections = get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert!(
            corrections.is_empty(),
            "スレッド追従はユーザー訂正ではないのでcorrection_logに書かない"
        );
    }

    #[test]
    fn test_auto_follow_uses_ai_assigned_by_with_no_confidence() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        auto_follow_threads(&conn, "acc1").unwrap();

        let info = get_assignment_info(&conn, "m2").unwrap().unwrap();
        assert_eq!(info.1, "ai");
        assert!(info.2.is_none(), "AI分類の確信度スコアと区別するためNone");
    }

    #[test]
    fn test_auto_follow_rolls_back_all_assignments_on_failure() {
        // 追従の書き込みは1トランザクション。途中で失敗したら、それまでに書き込んだ
        // 追従割り当てもロールバックされ「一部だけ追従された」状態を残さないこと
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        let mut m3 = make_mail("m3", "acc1", "Re: Test", "2026-04-13T12:00:00");
        m3.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        insert_mail(&conn, &m3);
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        // 失敗注入: スレッド内で最後に書き込まれる m3（date 最新）の INSERT だけ失敗させる。
        // これにより先行する m2 の INSERT は成功した後に失敗が起きる
        conn.execute_batch(
            "CREATE TRIGGER fail_follow_insert BEFORE INSERT ON mail_project_assignments
             WHEN NEW.mail_id = 'm3'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

        let result = auto_follow_threads(&conn, "acc1");
        assert!(result.is_err(), "注入した失敗がエラーとして伝播する");
        assert!(
            get_assignment_info(&conn, "m2").unwrap().is_none(),
            "先に書き込まれた m2 の追従もロールバックされる"
        );
        assert!(get_assignment_info(&conn, "m3").unwrap().is_none());
        assert!(
            get_assignment_info(&conn, "m1").unwrap().is_some(),
            "既存の割り当ては影響を受けない"
        );
    }

    // --- reject 後のスレッド追従復活防止（トゥームストーン） ---

    /// m1 割り当て済みスレッドの返信 m2 を追従 → 却下 → 再び一覧を開いても
    /// 追従で黙って復活しないこと
    #[test]
    fn test_auto_follow_does_not_revive_rejected_mail() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        // 1回目の追従で m2 が proj1 に入る
        assert_eq!(auto_follow_threads(&conn, "acc1").unwrap(), 1);
        assert!(get_assignment_info(&conn, "m2").unwrap().is_some());

        // ユーザーが m2 を却下（割り当て解除 + 除外トゥームストーン）
        reject_classification(&conn, "m2").unwrap();
        assert!(get_assignment_info(&conn, "m2").unwrap().is_none());

        // 一覧を開き直しても（＝再度追従を走らせても）m2 は復活しない
        assert_eq!(
            auto_follow_threads(&conn, "acc1").unwrap(),
            0,
            "却下したメールはスレッド追従で再割り当てされない"
        );
        assert!(
            get_assignment_info(&conn, "m2").unwrap().is_none(),
            "却下の意思が自動処理で黙って取り消されてはならない"
        );
    }

    /// 却下後もユーザーが手動で（move_mail_to_project）同じ案件へ割り当て直すのは許される
    #[test]
    fn test_manual_assign_after_reject_is_allowed() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();
        auto_follow_threads(&conn, "acc1").unwrap();
        reject_classification(&conn, "m2").unwrap();

        // ユーザーが手動で割り当て直す（D&D 相当）
        move_mail_to_project(&conn, "m2", "proj1").unwrap();
        let info = get_assignment_info(&conn, "m2").unwrap().unwrap();
        assert_eq!(info.0, "proj1", "手動割り当ては却下後も可能");
    }

    /// 手動で割り当て直すと除外トゥームストーンが解除され、以後は追従が再び有効になること
    #[test]
    fn test_manual_assign_clears_exclusion_and_reenables_follow() {
        let conn = setup_db();
        create_project(&conn, "proj1", "acc1", "Project Alpha");

        let mut m1 = make_mail("m1", "acc1", "Re: Test", "2026-04-13T10:00:00");
        m1.message_id = "<m1@example.com>".into();
        let mut m2 = make_mail("m2", "acc1", "Re: Test", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<m1@example.com>".into());
        insert_mail(&conn, &m1);
        insert_mail(&conn, &m2);
        assign_mail(&conn, "m1", "proj1", "user", Some(1.0)).unwrap();

        auto_follow_threads(&conn, "acc1").unwrap();
        reject_classification(&conn, "m2").unwrap();
        assert!(is_follow_excluded(&conn, "m2"), "却下で除外が付く");

        // ユーザーが気を変えて m2 を手動割り当て → 除外解除
        move_mail_to_project(&conn, "m2", "proj1").unwrap();
        assert!(
            !is_follow_excluded(&conn, "m2"),
            "手動割り当てで除外トゥームストーンが解除される（以後は追従の対象に戻る）"
        );
    }

    /// テスト補助: メールが追従除外トゥームストーンに載っているか
    fn is_follow_excluded(conn: &Connection, mail_id: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM follow_exclusions WHERE mail_id = ?1",
            params![mail_id],
            |row| row.get(0),
        )
        .unwrap()
    }
}
