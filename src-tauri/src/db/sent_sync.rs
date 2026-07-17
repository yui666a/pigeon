//! Sent フォルダ同期のマージ業務ロジック。
//!
//! 送信時にローカル保存した Sent 行（仮 uid・uid_confirmed=0）と、サーバーの
//! Sent フォルダから同期した行（実 uid）を message_id で突き合わせ、二重行を
//! 作らずに uid を後追い確定する。設計書:
//! docs/archive/specs/2026-07-12-sent-sync-uidplus-design.md
//!
//! 純粋な永続化 CRUD（insert/select/delete）は `db::mails` に置き、本モジュールは
//! 「どの行を残すか・いつスキップするか」という業務判断を担う。

use crate::db::mails::{delete_mail, get_mail_id_by_message_id, insert_mail, MAIL_COLUMNS};
use crate::error::AppError;
use crate::models::mail::Mail;
use rusqlite::{params, Connection, OptionalExtension};

/// 送信直後の Sent ローカル行を、フォルダ内 max(uid)+1 の仮 uid で挿入し、
/// 採番した uid を返す。
///
/// 採番と挿入を単一の INSERT ... SELECT 文で行うことで原子化する。
/// `get_max_uid + 1` → `insert_mail` の2段構えでは、間に並行する送信や
/// Sent 同期が同じ uid を採番して UNIQUE(account_id, folder, uid) 違反で
/// 挿入に失敗し得る（TOCTOU）。
///
/// `mail.uid` / `mail.uid_confirmed` は無視される: uid はこの関数が採番し、
/// 仮採番のため常に uid_confirmed=0 で保存する（Sent 同期の message_id マージで
/// サーバー実 uid へ後追い確定される。設計書 2026-07-12-sent-sync-uidplus-design.md）。
pub fn insert_sent_mail_with_next_uid(conn: &Connection, mail: &Mail) -> Result<u32, AppError> {
    conn.execute(
        &format!(
            "INSERT INTO mails ({})
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    (SELECT COALESCE(MAX(uid), 0) + 1 FROM mails WHERE account_id = ?2 AND folder = ?3),
                    ?16, ?17, ?18, ?19, 0",
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
            mail.flags,
            mail.is_read,
            mail.is_flagged,
            mail.fetched_at,
        ],
    )?;
    let uid: u32 = conn.query_row(
        "SELECT uid FROM mails WHERE id = ?1",
        params![mail.id],
        |row| row.get(0),
    )?;
    // v17 でトリガー廃止後は明示同期が必須（db::fts 冒頭コメント参照）。
    // uid はここで初めて採番されるが fts_mails には格納しないため、
    // 呼び出し側の `mail`（uid 確定前の値）をそのまま渡してよい。
    crate::db::fts::index_mail(conn, mail)?;
    Ok(uid)
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
        .optional()?;
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
///
/// 判定（SELECT）から書き込み（DELETE/UPDATE/INSERT）までを1トランザクションで包む。
/// 特に `merge_duplicate_sent_rows` は DELETE → UPDATE の複数書き込みのため、
/// 途中失敗で削除だけが残ると案件割り当ての保持という設計意図が崩れる。
pub fn upsert_sent_mail(conn: &Connection, mail: &Mail) -> Result<bool, AppError> {
    let tx = conn.unchecked_transaction()?;
    let inserted = upsert_sent_mail_in_tx(&tx, mail)?;
    tx.commit()?;
    Ok(inserted)
}

/// `upsert_sent_mail` の本体。呼び出し側でトランザクション境界を張ること。
fn upsert_sent_mail_in_tx(conn: &Connection, mail: &Mail) -> Result<bool, AppError> {
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
    // EXISTS は常に1行返すため「行なし」ケースはなく、エラーはそのまま伝播する
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mail_project_assignments WHERE mail_id = ?1)",
        params![mail_id],
        |row| row.get(0),
    )?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::mails::{get_mail_by_id, get_mails_by_account};
    use crate::test_helpers::{make_mail, setup_db};

    #[test]
    fn test_insert_sent_mail_with_next_uid_assigns_sequential_uids() {
        // 逐次呼び出しで uid が重複せず単調増加すること（B-6: 採番+挿入の原子化）
        let conn = setup_db();

        let mut m1 = make_mail("s1", "<s1@example.com>", "Sent 1", "2026-07-13T10:00:00");
        m1.folder = "Sent".into();
        let uid1 = insert_sent_mail_with_next_uid(&conn, &m1).unwrap();
        assert_eq!(uid1, 1, "空フォルダでは 1 から採番される");

        let mut m2 = make_mail("s2", "<s2@example.com>", "Sent 2", "2026-07-13T10:01:00");
        m2.folder = "Sent".into();
        let uid2 = insert_sent_mail_with_next_uid(&conn, &m2).unwrap();
        assert_eq!(uid2, 2, "連続採番で衝突しない");

        let mails = get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(mails.len(), 2);
    }

    #[test]
    fn test_insert_sent_mail_with_next_uid_continues_from_existing_max() {
        // 既存 max との関係: フォルダ内の最大 uid + 1 が採番されること
        let conn = setup_db();
        let mut existing = make_mail("s0", "<s0@example.com>", "Synced", "2026-07-13T09:00:00");
        existing.folder = "Sent".into();
        existing.uid = 41;
        insert_mail(&conn, &existing).unwrap();

        let mut mail = make_mail("s1", "<s1@example.com>", "Sent", "2026-07-13T10:00:00");
        mail.folder = "Sent".into();
        let uid = insert_sent_mail_with_next_uid(&conn, &mail).unwrap();
        assert_eq!(uid, 42);

        let loaded = get_mail_by_id(&conn, "s1").unwrap();
        assert_eq!(loaded.uid, 42, "採番された uid が永続化される");
    }

    #[test]
    fn test_insert_sent_mail_with_next_uid_ignores_caller_uid_and_marks_unconfirmed() {
        // 呼び出し側の uid 値は無視され、仮採番として uid_confirmed=0 で保存されること
        let conn = setup_db();
        let mut mail = make_mail("s1", "<s1@example.com>", "Sent", "2026-07-13T10:00:00");
        mail.folder = "Sent".into();
        mail.uid = 9999;
        mail.uid_confirmed = true;

        let uid = insert_sent_mail_with_next_uid(&conn, &mail).unwrap();
        assert_eq!(uid, 1, "mail.uid の値は採番に影響しない");

        let loaded = get_mail_by_id(&conn, "s1").unwrap();
        assert_eq!(loaded.uid, 1);
        assert!(
            !loaded.uid_confirmed,
            "採番 uid は推定値なので常に uid_confirmed=0"
        );
    }

    #[test]
    fn test_insert_sent_mail_with_next_uid_indexes_normalized_fts_row() {
        // Task 3 の配線漏れ修正対象: insert_sent_mail_with_next_uid は生の INSERT を
        // 行っており db::fts::index_mail を呼んでいなかったため、送信メールが
        // fts_mails に索引されず検索不能になっていた（v17 でトリガー廃止後の退行）。
        let conn = setup_db();
        let mut mail = make_mail(
            "s1",
            "<s1@example.com>",
            "ＳＡＴＯ確認",
            "2026-07-13T10:00:00",
        );
        mail.folder = "Sent".into();

        insert_sent_mail_with_next_uid(&conn, &mail).unwrap();

        let subject: String = conn
            .query_row(
                "SELECT subject FROM fts_mails WHERE mail_id = ?1",
                params!["s1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            subject, "sato確認",
            "送信メールも他の書き込み経路と同様に正規化済みで fts_mails に索引されること"
        );
    }

    #[test]
    fn test_insert_sent_mail_with_next_uid_is_scoped_per_folder() {
        // 他フォルダの uid は採番に影響しないこと
        let conn = setup_db();
        let mut inbox = make_mail("i1", "<i1@example.com>", "Inbox", "2026-07-13T09:00:00");
        inbox.uid = 100; // folder=INBOX
        insert_mail(&conn, &inbox).unwrap();

        let mut mail = make_mail("s1", "<s1@example.com>", "Sent", "2026-07-13T10:00:00");
        mail.folder = "Sent".into();
        let uid = insert_sent_mail_with_next_uid(&conn, &mail).unwrap();
        assert_eq!(uid, 1, "INBOX の max uid は Sent の採番に影響しない");
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
    fn test_upsert_sent_mail_merge_rolls_back_atomically_on_mid_failure() {
        // B-1: 重複統合は DELETE（占有行の削除）→ UPDATE（uid 確定）の複数書き込み。
        // 途中で失敗したときに DELETE だけが残る中途半端な状態にならないこと
        // （案件割り当ての保持を最重要視する設計意図のため、全体がロールバックされる）。
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

        // 同一メールの重複行が確定先 uid=5000 を占有（統合で削除される側）
        let mut occupant = make_mail("s_dup", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        occupant.folder = "Sent".into();
        occupant.uid = 5000;
        occupant.uid_confirmed = true;
        insert_mail(&conn, &occupant).unwrap();

        // 失敗注入: 統合の2番目の書き込み（s1 の uid 確定 UPDATE）だけを失敗させる
        conn.execute_batch(
            "CREATE TRIGGER fail_confirm BEFORE UPDATE OF uid ON mails
             WHEN NEW.id = 's1' AND NEW.uid = 5000
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

        let mut server = make_mail("s_srv", "<mid@pigeon.local>", "件名", "2026-07-12T10:00:00");
        server.folder = "Sent".into();
        server.uid = 5000;
        let result = upsert_sent_mail(&conn, &server);
        assert!(result.is_err(), "注入した失敗がエラーとして伝播する");

        // ロールバックにより、先行する DELETE（s_dup の削除）も取り消されていること
        let all = get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(all.len(), 2, "途中失敗では部分的な書き込みが残らない");
        assert!(
            get_mail_by_id(&conn, "s_dup").is_ok(),
            "削除がロールバックされる"
        );
        let s1 = get_mail_by_id(&conn, "s1").unwrap();
        assert_eq!(s1.uid, 1, "s1 も元のまま");
        assert!(!s1.uid_confirmed);
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
        let mut foreign = make_mail(
            "s_foreign",
            "<foreign@server>",
            "他人",
            "2026-07-12T09:00:00",
        );
        foreign.folder = "Sent".into();
        foreign.uid = 5000;
        insert_mail(&conn, &foreign).unwrap();

        // s1 を uid=5000 に確定しようとするが別メール占有 → スキップ（Err にしない）
        let mut server = make_mail(
            "s_srv",
            "<mine@pigeon.local>",
            "自分",
            "2026-07-12T10:00:00",
        );
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
        let mut server = make_mail(
            "s1",
            "<external@server>",
            "他クライアント",
            "2026-07-12T10:00:00",
        );
        server.folder = "Sent".into();
        server.uid = 777;
        let inserted = upsert_sent_mail(&conn, &server).unwrap();
        assert!(inserted, "未知の message_id は挿入される");

        let all = get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].uid, 777);
    }
}
