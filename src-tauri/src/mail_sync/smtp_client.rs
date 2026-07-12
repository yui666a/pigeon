//! SMTP送信 (lettre)。
//! メッセージ構築は純関数として切り出し、実送信 (`send`) と分離してテスト可能にする。

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::error::AppError;
use crate::models::account::AuthType;

/// 送信メッセージの構築入力。lettre型に依存しない純データ
#[derive(Debug, Clone)]
pub struct OutgoingMail {
    pub from_name: String,
    pub from_email: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// `<uuid@pigeon.local>` 形式。ローカルDBに保存する message_id と一致させる
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// ローカルDBと送信メッセージで共有する Message-ID を生成する
pub fn generate_message_id() -> String {
    format!("<{}@pigeon.local>", uuid::Uuid::new_v4())
}

/// 返信時の References ヘッダーを構築する（RFC 5322）。
/// 元メールの References に元メールの Message-ID を連結する。
/// 元メールに References が無ければ Message-ID 単独。既に含まれていれば重複させない
pub fn build_references(orig_references: Option<&str>, orig_message_id: &str) -> String {
    match orig_references {
        Some(refs) if !refs.trim().is_empty() => {
            let refs = refs.trim();
            if refs.contains(orig_message_id) {
                refs.to_string()
            } else {
                format!("{} {}", refs, orig_message_id)
            }
        }
        _ => orig_message_id.to_string(),
    }
}

/// メールアドレス文字列を lettre Mailbox にパースする（`Name <a@b>` 形式も可）
fn parse_mailbox(addr: &str) -> Result<Mailbox, AppError> {
    addr.trim()
        .parse::<Mailbox>()
        .map_err(|e| AppError::Validation(format!("不正なメールアドレス: {} ({})", addr, e)))
}

/// OutgoingMail から lettre Message を構築する。
/// To が空、またはアドレスが不正な場合は Validation エラー
pub fn build_message(mail: &OutgoingMail) -> Result<Message, AppError> {
    if mail.to.iter().all(|a| a.trim().is_empty()) {
        return Err(AppError::Validation("宛先(To)が指定されていません".into()));
    }

    let from_addr = mail.from_email.parse::<lettre::Address>().map_err(|e| {
        AppError::Validation(format!("不正な送信元アドレス: {} ({})", mail.from_email, e))
    })?;
    let from_name = if mail.from_name.trim().is_empty() {
        None
    } else {
        Some(mail.from_name.trim().to_string())
    };

    let mut builder = Message::builder()
        .from(Mailbox::new(from_name, from_addr))
        .subject(mail.subject.clone())
        .message_id(Some(mail.message_id.clone()))
        .date_now();

    for to in mail.to.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.to(parse_mailbox(to)?);
    }
    for cc in mail.cc.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.cc(parse_mailbox(cc)?);
    }
    for bcc in mail.bcc.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.bcc(parse_mailbox(bcc)?);
    }
    if let Some(irt) = &mail.in_reply_to {
        builder = builder.in_reply_to(irt.clone());
    }
    if let Some(refs) = &mail.references {
        builder = builder.references(refs.clone());
    }

    builder
        .header(ContentType::TEXT_PLAIN)
        .body(mail.body_text.clone())
        .map_err(|e| AppError::Smtp(format!("メッセージ構築に失敗: {}", e)))
}

/// SMTP送信。ポート465はImplicit TLS、それ以外はSTARTTLS。
/// 認証は AuthType に応じて PLAIN/LOGIN または XOAUTH2
pub async fn send(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
    message: Message,
) -> Result<(), AppError> {
    let mechanisms = match auth_type {
        AuthType::Plain => vec![Mechanism::Plain, Mechanism::Login],
        AuthType::Oauth2 => vec![Mechanism::Xoauth2],
    };

    let builder = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
    }
    .map_err(|e| AppError::Smtp(format!("SMTP接続設定に失敗: {}", e)))?;

    let transport = builder
        .port(port)
        .credentials(Credentials::new(
            username.to_string(),
            credential.to_string(),
        ))
        .authentication(mechanisms)
        .timeout(Some(Duration::from_secs(30)))
        .build();

    tokio::time::timeout(Duration::from_secs(30), transport.send(message))
        .await
        .map_err(|_| AppError::Smtp("SMTP送信がタイムアウトしました (30s)".into()))?
        .map_err(|e| AppError::Smtp(format!("SMTP送信に失敗: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_mail() -> OutgoingMail {
        OutgoingMail {
            from_name: "Hiroshi".into(),
            from_email: "me@example.com".into(),
            to: vec!["tanaka@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "テスト件名".into(),
            body_text: "こんにちは".into(),
            message_id: "<abc-123@pigeon.local>".into(),
            in_reply_to: None,
            references: None,
        }
    }

    #[test]
    fn test_generate_message_id_format() {
        let id = generate_message_id();
        assert!(id.starts_with('<'));
        assert!(id.ends_with("@pigeon.local>"));
        // 2回呼ぶと異なるIDになる
        assert_ne!(id, generate_message_id());
    }

    #[test]
    fn test_build_references_no_orig_references() {
        assert_eq!(build_references(None, "<a@ex.com>"), "<a@ex.com>");
        assert_eq!(build_references(Some("  "), "<a@ex.com>"), "<a@ex.com>");
    }

    #[test]
    fn test_build_references_appends_message_id() {
        assert_eq!(
            build_references(Some("<root@ex.com> <mid@ex.com>"), "<a@ex.com>"),
            "<root@ex.com> <mid@ex.com> <a@ex.com>"
        );
    }

    #[test]
    fn test_build_references_no_duplicate() {
        assert_eq!(
            build_references(Some("<root@ex.com> <a@ex.com>"), "<a@ex.com>"),
            "<root@ex.com> <a@ex.com>"
        );
    }

    #[test]
    fn test_build_message_basic_headers() {
        let msg = build_message(&base_mail()).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(
            raw.contains("From: \"Hiroshi\" <me@example.com>")
                || raw.contains("From: Hiroshi <me@example.com>")
        );
        assert!(raw.contains("To: <tanaka@example.com>") || raw.contains("To: tanaka@example.com"));
        assert!(raw.contains("Message-ID: <abc-123@pigeon.local>"));
        assert!(raw.contains("Subject:"));
    }

    #[test]
    fn test_build_message_reply_headers() {
        let mut mail = base_mail();
        mail.in_reply_to = Some("<orig@ex.com>".into());
        mail.references = Some("<root@ex.com> <orig@ex.com>".into());
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("In-Reply-To: <orig@ex.com>"));
        assert!(raw.contains("References: <root@ex.com> <orig@ex.com>"));
    }

    #[test]
    fn test_build_message_multiple_recipients() {
        let mut mail = base_mail();
        mail.to = vec!["a@ex.com".into(), "b@ex.com".into()];
        mail.cc = vec!["c@ex.com".into()];
        mail.bcc = vec!["d@ex.com".into()];
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("a@ex.com"));
        assert!(raw.contains("b@ex.com"));
        assert!(raw.contains("Cc:"));
        // Bcc はヘッダーに含まれない（envelope のみ）が、エラーにはならない
    }

    #[test]
    fn test_build_message_empty_to_rejected() {
        let mut mail = base_mail();
        mail.to = vec![];
        assert!(matches!(build_message(&mail), Err(AppError::Validation(_))));
        let mut mail2 = base_mail();
        mail2.to = vec!["  ".into()];
        assert!(matches!(
            build_message(&mail2),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn test_build_message_invalid_address_rejected() {
        let mut mail = base_mail();
        mail.to = vec!["not-an-address".into()];
        assert!(matches!(build_message(&mail), Err(AppError::Validation(_))));
    }

    #[test]
    fn test_build_message_from_name_empty_ok() {
        let mut mail = base_mail();
        mail.from_name = "".into();
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("me@example.com"));
    }

    #[test]
    fn test_body_is_included() {
        let msg = build_message(&base_mail()).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        // UTF-8本文はbase64またはquoted-printableでエンコードされる場合があるため、
        // ASCII本文で内容が残ることを別途確認する
        let mut ascii = base_mail();
        ascii.body_text = "hello world".into();
        let raw_ascii = String::from_utf8(build_message(&ascii).unwrap().formatted()).unwrap();
        assert!(raw_ascii.contains("hello world"));
        assert!(!raw.is_empty());
    }
}
