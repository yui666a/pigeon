//! メール送信 command。
//! 検証 → メッセージ構築 → SMTP送信 → Sentフォルダ保存 → ローカルDB挿入。
//! 設計: docs/superpowers/specs/2026-07-12-mail-send-design.md

use serde::Deserialize;
use tauri::State;

use crate::db::{accounts, mails, settings};
use crate::error::AppError;
use crate::mail_sync::smtp_client::OutgoingMail;
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

/// リクエストとアカウント情報から OutgoingMail を構築する
pub(crate) fn build_outgoing(
    account: &Account,
    req: &SendMailRequest,
    reply_source: Option<&Mail>,
    message_id: &str,
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
        message_id: message_id.to_string(),
        in_reply_to,
        references,
    }
}

/// 送信済みメールのローカルDBレコードを構築する（folder='Sent'）。
/// uid は同フォルダ内の max_uid + 1（UNIQUE(account_id, folder, uid) を満たすため）
pub(crate) fn build_sent_record(
    account: &Account,
    outgoing: &OutgoingMail,
    uid: u32,
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
        body_html: None,
        date: now.clone(),
        has_attachments: false,
        raw_size: Some(raw_size as i64),
        uid,
        flags: Some("\\Seen".into()),
        // 自分が送ったメールは常に既読
        is_read: true,
        is_flagged: false,
        fetched_at: now,
        // 送信時の uid は get_max_uid+1 の推定値。Sent 同期で後追い確定するまで未確定
        uid_confirmed: false,
    }
}

#[tauri::command]
pub async fn send_mail(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    req: SendMailRequest,
) -> Result<(), AppError> {
    // 1. アカウントと返信元メールを取得
    let (account, reply_source, sent_folder) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let account = accounts::get_account(&conn, &req.account_id)?;
        let reply_source = match &req.reply_to_mail_id {
            Some(id) => Some(mails::get_mail_by_id(&conn, id)?),
            None => None,
        };
        let sent_folder = settings::get_or_default(&conn, "sent_folder", "Sent");
        (account, reply_source, sent_folder)
    };

    // 2. メッセージ構築（バリデーション含む）
    let message_id = smtp_client::generate_message_id();
    let outgoing = build_outgoing(&account, &req, reply_source.as_ref(), &message_id);
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

    // 6. ローカルDBに挿入（FTS5はトリガーで自動反映）
    {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let uid = mails::get_max_uid(&conn, &account.id, "Sent")? + 1;
        let record = build_sent_record(&account, &outgoing, uid, raw.len());
        mails::insert_mail(&conn, &record)?;
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
        let outgoing = build_outgoing(&account, &req, None, "<mid@pigeon.local>");
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
        let outgoing = build_outgoing(&account, &req, None, "<mid@pigeon.local>");
        let record = build_sent_record(&account, &outgoing, 5, 1234);
        assert_eq!(record.folder, "Sent");
        assert_eq!(record.uid, 5);
        assert_eq!(record.message_id, "<mid@pigeon.local>");
        assert_eq!(record.from_addr, "me@example.com");
        assert_eq!(record.to_addr, "tanaka@example.com");
        assert_eq!(record.cc_addr.as_deref(), Some("c1@ex.com, c2@ex.com"));
        assert_eq!(record.flags.as_deref(), Some("\\Seen"));
        assert_eq!(record.raw_size, Some(1234));
        assert_eq!(record.body_text.as_deref(), Some("本文"));
    }

    #[test]
    fn test_build_sent_record_empty_cc_is_none() {
        let account = make_account();
        let outgoing = build_outgoing(&account, &make_request(), None, "<mid@pigeon.local>");
        let record = build_sent_record(&account, &outgoing, 1, 100);
        assert!(record.cc_addr.is_none());
    }

    #[test]
    fn test_sent_record_roundtrips_through_db() {
        // 挿入 → 取得で同じ内容が得られ、uid の採番が単調増加すること
        let conn = crate::test_helpers::setup_db();
        let account = make_account();
        let outgoing = build_outgoing(&account, &make_request(), None, "<mid1@pigeon.local>");

        let uid1 = mails::get_max_uid(&conn, "acc1", "Sent").unwrap() + 1;
        let rec1 = build_sent_record(&account, &outgoing, uid1, 100);
        assert!(mails::insert_mail(&conn, &rec1).unwrap());

        let uid2 = mails::get_max_uid(&conn, "acc1", "Sent").unwrap() + 1;
        assert_eq!(uid2, uid1 + 1);
        let outgoing2 = OutgoingMail {
            message_id: "<mid2@pigeon.local>".into(),
            ..outgoing
        };
        let rec2 = build_sent_record(&account, &outgoing2, uid2, 100);
        assert!(mails::insert_mail(&conn, &rec2).unwrap());

        let loaded = mails::get_mail_by_id(&conn, &rec1.id).unwrap();
        assert_eq!(loaded.message_id, "<mid1@pigeon.local>");
        assert_eq!(loaded.folder, "Sent");
    }
}
