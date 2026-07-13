//! メール送信 command。
//! 検証 → メッセージ構築 → SMTP送信 → Sentフォルダ保存 → ローカルDB挿入。
//! 設計: docs/superpowers/specs/2026-07-12-mail-send-design.md

use serde::Deserialize;
use tauri::State;

use crate::db::{accounts, mails, settings};
use crate::error::AppError;
use crate::mail_sync::smtp_client::{OutgoingAttachment, OutgoingMail};
use crate::mail_sync::{imap_client, smtp_client};
use crate::models::account::{Account, AccountProvider};
use crate::models::mail::Mail;
use crate::state::{DbState, SecureStoreState};

#[derive(Debug, Clone, Deserialize)]
pub struct SendMailRequest {
    pub account_id: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// 返信元メールのローカルID。In-Reply-To / References の導出に使う。
    /// 新規・転送では None
    #[serde(default)]
    pub reply_to_mail_id: Option<String>,
    /// リッチ本文の HTML。Some なら multipart/alternative で送る（plain は HTML から生成）。
    /// None ならプレーンのみ送信
    #[serde(default)]
    pub body_html: Option<String>,
    /// 添付ファイルの絶対パス。空なら添付なし
    #[serde(default)]
    pub attachments: Vec<String>,
}

/// 添付候補ファイルのサイズ（バイト）を返す。
/// フロントは合計サイズの表示・超過警告に使う（送信時の上限検証は Rust の
/// build_message が担う二重防御。設計書 2026-07-13-rich-compose-design.md）。
/// plugin-fs を導入せず、既存の「パスを Rust へ渡す」方式に揃えた command
#[tauri::command]
pub fn stat_file(path: String) -> Result<u64, AppError> {
    std::fs::metadata(&path)
        .map(|m| m.len())
        .map_err(|e| AppError::FileIo(format!("ファイル情報の取得に失敗 {}: {}", path, e)))
}

/// 返信元メールから (In-Reply-To, References) を導出する
pub(crate) fn derive_threading_headers(
    reply_source: Option<&Mail>,
) -> (Option<String>, Option<String>) {
    match reply_source {
        Some(orig) => (
            Some(orig.message_id.clone()),
            Some(smtp_client::build_references(
                orig.references.as_deref(),
                &orig.message_id,
            )),
        ),
        None => (None, None),
    }
}

/// 添付パスのリストを読み込み、OutgoingAttachment のリストに変換する。
/// ファイル名は末尾コンポーネント、Content-Type は拡張子から推定する
pub(crate) fn read_attachments(paths: &[String]) -> Result<Vec<OutgoingAttachment>, AppError> {
    paths
        .iter()
        .map(|path| {
            let data = std::fs::read(path).map_err(|e| {
                AppError::FileIo(format!("添付ファイルの読み込みに失敗 {}: {}", path, e))
            })?;
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment")
                .to_string();
            let content_type = smtp_client::guess_content_type(&filename);
            Ok(OutgoingAttachment {
                filename,
                content_type,
                data,
            })
        })
        .collect()
}

/// リクエストとアカウント情報から OutgoingMail を構築する。
/// 添付は読み込み済みのものを渡す（IO は read_attachments に分離）
pub(crate) fn build_outgoing(
    account: &Account,
    req: &SendMailRequest,
    reply_source: Option<&Mail>,
    message_id: &str,
    attachments: Vec<OutgoingAttachment>,
) -> OutgoingMail {
    let (in_reply_to, references) = derive_threading_headers(reply_source);
    OutgoingMail {
        from_name: account.name.clone(),
        from_email: account.email.clone(),
        to: req.to.clone(),
        cc: req.cc.clone(),
        bcc: req.bcc.clone(),
        subject: req.subject.clone(),
        body_text: req.body_text.clone(),
        body_html: req.body_html.clone(),
        attachments,
        message_id: message_id.to_string(),
        in_reply_to,
        references,
    }
}

/// 送信済みメールのローカルDBレコードを構築する（folder='Sent'）。
/// uid はここでは 0 のプレースホルダ。挿入時に `insert_sent_mail_with_next_uid` が
/// フォルダ内 max(uid)+1 を原子的に採番する（TOCTOU による UNIQUE 衝突の防止）
pub(crate) fn build_sent_record(
    account: &Account,
    outgoing: &OutgoingMail,
    raw_size: usize,
) -> Mail {
    let now = chrono::Utc::now().to_rfc3339();
    Mail {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: account.id.clone(),
        folder: "Sent".into(),
        message_id: outgoing.message_id.clone(),
        in_reply_to: outgoing.in_reply_to.clone(),
        references: outgoing.references.clone(),
        from_addr: account.email.clone(),
        to_addr: outgoing.to.join(", "),
        cc_addr: if outgoing.cc.is_empty() {
            None
        } else {
            Some(outgoing.cc.join(", "))
        },
        subject: outgoing.subject.clone(),
        body_text: Some(outgoing.body_text.clone()),
        body_html: outgoing.body_html.clone(),
        date: now.clone(),
        has_attachments: !outgoing.attachments.is_empty(),
        raw_size: Some(raw_size as i64),
        // 挿入時に insert_sent_mail_with_next_uid が採番するプレースホルダ
        uid: 0,
        flags: Some("\\Seen".into()),
        // 自分が送ったメールは常に既読
        is_read: true,
        is_flagged: false,
        fetched_at: now,
        // 送信時の uid は max_uid+1 の推定値。Sent 同期で後追い確定するまで未確定
        uid_confirmed: false,
    }
}

/// SMTP 送信成功後のローカル Sent 反映（ベストエフォート）。
/// この時点で送信自体は完了しているため、DB挿入の失敗を Err として伝播しない。
/// Err を返すとUIが「送信失敗」を表示し、ユーザーの再送で二重送信になるため、
/// 失敗は警告ログに留める。未反映分は次回 Sent 同期の message_id マージ
/// （`upsert_sent_mail`）でサーバーの Sent フォルダから復元される
pub(crate) fn persist_sent_local_best_effort(
    conn: &rusqlite::Connection,
    account: &Account,
    outgoing: &OutgoingMail,
    raw_size: usize,
) {
    let record = build_sent_record(account, outgoing, raw_size);
    if let Err(e) = mails::insert_sent_mail_with_next_uid(conn, &record) {
        eprintln!(
            "[warn] mail sent but local Sent insert failed (message_id={}): {}",
            outgoing.message_id, e
        );
    }
}

#[tauri::command]
pub async fn send_mail(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    req: SendMailRequest,
) -> Result<(), AppError> {
    // 1. アカウントと返信元メールを取得
    let (account, reply_source, sent_folder) = state.with_conn(|conn| {
        let account = accounts::get_account(conn, &req.account_id)?;
        let reply_source = match &req.reply_to_mail_id {
            Some(id) => Some(mails::get_mail_by_id(conn, id)?),
            None => None,
        };
        let sent_folder = settings::get_or_default(conn, "sent_folder", "Sent")?;
        Ok((account, reply_source, sent_folder))
    })?;

    // 2. 添付ファイルの読み込み → メッセージ構築（サイズ・アドレス検証含む）
    let attachments = read_attachments(&req.attachments)?;
    let message_id = smtp_client::generate_message_id();
    let outgoing = build_outgoing(
        &account,
        &req,
        reply_source.as_ref(),
        &message_id,
        attachments,
    );
    let message = smtp_client::build_message(&outgoing)?;
    let raw = message.formatted();

    // 3. 認証情報の解決（IMAPと共通。OAuthリフレッシュ込み）
    let (auth_type, username, credential) =
        crate::commands::mail_commands::resolve_imap_credentials(&account, &secure_store.0).await?;

    // 4. SMTP送信
    smtp_client::send(
        &account.smtp_host,
        account.smtp_port,
        &auth_type,
        &username,
        &credential,
        message,
    )
    .await?;

    // 5. Sentフォルダへ保存。
    //    Google: GmailがSMTP送信時に自動保存するためAPPENDしない（二重保存防止）。
    //    Other:  ベストエフォートでAPPEND（送信自体は完了しているため失敗しても成功扱い）
    if account.provider == AccountProvider::Other {
        if let Err(e) = imap_client::append_message(
            &account.imap_host,
            account.imap_port,
            &auth_type,
            &username,
            &credential,
            &sent_folder,
            &raw,
        )
        .await
        {
            eprintln!("[warn] Sent folder append failed: {}", e);
        }
    }

    // 6. ローカルDBに挿入（FTS5はトリガーで自動反映）。
    //    SMTP送信は成功しているためベストエフォート: ロック取得を含め失敗しても
    //    Err を返さない（persist_sent_local_best_effort のドキュメント参照）
    let persisted = state.with_conn(|conn| {
        persist_sent_local_best_effort(conn, &account, &outgoing, raw.len());
        Ok(())
    });
    if let Err(e) = persisted {
        eprintln!(
            "[warn] mail sent but local Sent insert skipped (DB lock failed): {}",
            e
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::account::{AccountProvider, AuthType};
    use crate::test_helpers::make_mail;

    fn make_account() -> Account {
        Account {
            id: "acc1".into(),
            name: "Hiroshi".into(),
            email: "me@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Plain,
            provider: AccountProvider::Other,
            created_at: "2026-07-12T00:00:00Z".into(),
            needs_reauth: false,
        }
    }

    fn make_request() -> SendMailRequest {
        SendMailRequest {
            account_id: "acc1".into(),
            to: vec!["tanaka@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "件名".into(),
            body_text: "本文".into(),
            reply_to_mail_id: None,
            body_html: None,
            attachments: vec![],
        }
    }

    #[test]
    fn test_derive_threading_headers_new_mail() {
        let (irt, refs) = derive_threading_headers(None);
        assert!(irt.is_none());
        assert!(refs.is_none());
    }

    #[test]
    fn test_derive_threading_headers_reply() {
        let mut orig = make_mail("m1", "<orig@ex.com>", "Hello", "2026-07-12T10:00:00");
        orig.references = Some("<root@ex.com>".into());
        let (irt, refs) = derive_threading_headers(Some(&orig));
        assert_eq!(irt.as_deref(), Some("<orig@ex.com>"));
        assert_eq!(refs.as_deref(), Some("<root@ex.com> <orig@ex.com>"));
    }

    #[test]
    fn test_derive_threading_headers_reply_without_references() {
        let orig = make_mail("m1", "<orig@ex.com>", "Hello", "2026-07-12T10:00:00");
        let (irt, refs) = derive_threading_headers(Some(&orig));
        assert_eq!(irt.as_deref(), Some("<orig@ex.com>"));
        assert_eq!(refs.as_deref(), Some("<orig@ex.com>"));
    }

    #[test]
    fn test_build_outgoing_uses_account_identity() {
        let account = make_account();
        let req = make_request();
        let outgoing = build_outgoing(&account, &req, None, "<mid@pigeon.local>", vec![]);
        assert_eq!(outgoing.from_name, "Hiroshi");
        assert_eq!(outgoing.from_email, "me@example.com");
        assert_eq!(outgoing.message_id, "<mid@pigeon.local>");
        assert!(outgoing.in_reply_to.is_none());
    }

    #[test]
    fn test_build_sent_record_fields() {
        let account = make_account();
        let req = SendMailRequest {
            cc: vec!["c1@ex.com".into(), "c2@ex.com".into()],
            ..make_request()
        };
        let outgoing = build_outgoing(&account, &req, None, "<mid@pigeon.local>", vec![]);
        let record = build_sent_record(&account, &outgoing, 1234);
        assert_eq!(record.folder, "Sent");
        assert_eq!(record.uid, 0, "uid は挿入時採番のプレースホルダ");
        assert!(!record.uid_confirmed);
        assert_eq!(record.message_id, "<mid@pigeon.local>");
        assert_eq!(record.from_addr, "me@example.com");
        assert_eq!(record.to_addr, "tanaka@example.com");
        assert_eq!(record.cc_addr.as_deref(), Some("c1@ex.com, c2@ex.com"));
        assert_eq!(record.flags.as_deref(), Some("\\Seen"));
        assert_eq!(record.raw_size, Some(1234));
        assert_eq!(record.body_text.as_deref(), Some("本文"));
    }

    #[test]
    fn test_build_sent_record_reflects_html_and_attachments() {
        let account = make_account();
        let req = SendMailRequest {
            body_html: Some("<p>hi</p>".into()),
            ..make_request()
        };
        let att = OutgoingAttachment {
            filename: "a.pdf".into(),
            content_type: "application/pdf".into(),
            data: vec![0u8; 3],
        };
        let outgoing = build_outgoing(&account, &req, None, "<mid@pigeon.local>", vec![att]);
        let record = build_sent_record(&account, &outgoing, 100);
        assert_eq!(record.body_html.as_deref(), Some("<p>hi</p>"));
        assert!(record.has_attachments);
    }

    #[test]
    fn test_read_attachments_reads_bytes_and_infers_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"%PDF-1.4").unwrap();
        let atts = read_attachments(&[path.to_string_lossy().to_string()]).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].filename, "report.pdf");
        assert_eq!(atts[0].content_type, "application/pdf");
        assert_eq!(atts[0].data, b"%PDF-1.4");
    }

    #[test]
    fn test_stat_file_returns_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"12345").unwrap();
        let size = stat_file(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(size, 5);
    }

    #[test]
    fn test_stat_file_missing_errors() {
        assert!(matches!(
            stat_file("/nonexistent/path/x.bin".into()),
            Err(AppError::FileIo(_))
        ));
    }

    #[test]
    fn test_read_attachments_missing_file_errors() {
        let result = read_attachments(&["/nonexistent/path/x.bin".into()]);
        assert!(matches!(result, Err(AppError::FileIo(_))));
    }

    #[test]
    fn test_build_sent_record_empty_cc_is_none() {
        let account = make_account();
        let outgoing = build_outgoing(
            &account,
            &make_request(),
            None,
            "<mid@pigeon.local>",
            vec![],
        );
        let record = build_sent_record(&account, &outgoing, 100);
        assert!(record.cc_addr.is_none());
    }

    #[test]
    fn test_persist_sent_local_best_effort_inserts_record() {
        let conn = crate::test_helpers::setup_db();
        let account = make_account();
        let outgoing = build_outgoing(
            &account,
            &make_request(),
            None,
            "<mid@pigeon.local>",
            vec![],
        );

        persist_sent_local_best_effort(&conn, &account, &outgoing, 100);

        let mails = mails::get_mails_by_account(&conn, "acc1", "Sent").unwrap();
        assert_eq!(mails.len(), 1);
        assert_eq!(mails[0].message_id, "<mid@pigeon.local>");
        assert_eq!(mails[0].uid, 1);
        assert!(!mails[0].uid_confirmed);
    }

    #[test]
    fn test_persist_sent_local_best_effort_swallows_db_failure() {
        // B-7: SMTP送信成功後のローカルDB挿入失敗はErrにしない（再送による二重送信防止）。
        // mailsテーブルを壊してDB失敗を誘発しても、panicもErr伝播もしないこと
        let conn = crate::test_helpers::setup_db();
        conn.execute_batch("DROP TABLE mails").unwrap();
        let account = make_account();
        let outgoing = build_outgoing(
            &account,
            &make_request(),
            None,
            "<mid@pigeon.local>",
            vec![],
        );

        persist_sent_local_best_effort(&conn, &account, &outgoing, 100);
    }

    #[test]
    fn test_sent_record_roundtrips_through_db() {
        // 挿入 → 取得で同じ内容が得られ、uid が原子的採番で単調増加すること
        let conn = crate::test_helpers::setup_db();
        let account = make_account();
        let outgoing = build_outgoing(
            &account,
            &make_request(),
            None,
            "<mid1@pigeon.local>",
            vec![],
        );

        let rec1 = build_sent_record(&account, &outgoing, 100);
        let uid1 = mails::insert_sent_mail_with_next_uid(&conn, &rec1).unwrap();

        let outgoing2 = OutgoingMail {
            message_id: "<mid2@pigeon.local>".into(),
            ..outgoing
        };
        let rec2 = build_sent_record(&account, &outgoing2, 100);
        let uid2 = mails::insert_sent_mail_with_next_uid(&conn, &rec2).unwrap();
        assert_eq!(uid2, uid1 + 1);

        let loaded = mails::get_mail_by_id(&conn, &rec1.id).unwrap();
        assert_eq!(loaded.message_id, "<mid1@pigeon.local>");
        assert_eq!(loaded.folder, "Sent");
        assert_eq!(loaded.uid, uid1);
    }
}
