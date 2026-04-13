use mail_parser::MessageParser;
use uuid::Uuid;

use crate::models::mail::Mail;

pub fn parse_mime(raw: &[u8], account_id: &str, folder: &str, uid: u32) -> Option<Mail> {
    let message = MessageParser::default().parse(raw)?;

    let message_id = message
        .message_id()
        .map(|s| format!("<{}>", s))
        .unwrap_or_else(|| format!("<generated-{}@pigeon>", Uuid::new_v4()));

    let in_reply_to = message.in_reply_to().as_text().map(|s| format!("<{}>", s));

    let references = {
        let refs = message.references();
        refs.as_text_list()
            .map(|list| {
                list.iter()
                    .map(|s| format!("<{}>", s))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .or_else(|| refs.as_text().map(|s| format!("<{}>", s)))
    };

    let from_addr = message
        .from()
        .and_then(|a| a.first())
        .map(|a| {
            if let Some(name) = a.name() {
                format!("{} <{}>", name, a.address().unwrap_or(""))
            } else {
                a.address().unwrap_or("").to_string()
            }
        })
        .unwrap_or_default();

    let to_addr = message
        .to()
        .and_then(|a| a.first())
        .map(|a| a.address().unwrap_or("").to_string())
        .unwrap_or_default();

    let cc_addr = message.cc().and_then(|addrs| {
        let cc: Vec<String> = addrs
            .iter()
            .filter_map(|a| a.address().map(|s| s.to_string()))
            .collect();
        if cc.is_empty() {
            None
        } else {
            Some(cc.join(", "))
        }
    });

    let subject = message.subject().unwrap_or("(no subject)").to_string();
    let body_text = message.body_text(0).map(|s| s.to_string());
    let body_html = message.body_html(0).map(|s| s.to_string());

    let date = message
        .date()
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let has_attachments = message.attachment_count() > 0;

    Some(Mail {
        id: Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        folder: folder.to_string(),
        message_id,
        in_reply_to,
        references,
        from_addr,
        to_addr,
        cc_addr,
        subject,
        body_text,
        body_html,
        date,
        has_attachments,
        raw_size: Some(raw.len() as i64),
        uid,
        flags: None,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_EMAIL: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: Test Email\r\n\
        Message-ID: <test123@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Hello, this is a test email.";

    const REPLY_EMAIL: &[u8] = b"From: recipient@example.com\r\n\
        To: sender@example.com\r\n\
        Subject: Re: Test Email\r\n\
        Message-ID: <reply456@example.com>\r\n\
        In-Reply-To: <test123@example.com>\r\n\
        References: <test123@example.com>\r\n\
        Date: Mon, 13 Apr 2026 11:00:00 +0900\r\n\
        \r\n\
        Thanks for the test.";

    #[test]
    fn test_parse_simple_email() {
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1).unwrap();
        assert_eq!(mail.subject, "Test Email");
        assert_eq!(mail.from_addr, "sender@example.com");
        assert_eq!(mail.to_addr, "recipient@example.com");
        assert_eq!(mail.message_id, "<test123@example.com>");
        assert!(mail.in_reply_to.is_none());
        assert!(mail.body_text.unwrap().contains("Hello"));
    }

    #[test]
    fn test_parse_reply_email() {
        let mail = parse_mime(REPLY_EMAIL, "acc1", "INBOX", 2).unwrap();
        assert_eq!(mail.subject, "Re: Test Email");
        assert!(mail.in_reply_to.is_some());
        assert!(mail.references.is_some());
    }

    #[test]
    fn test_parse_invalid_does_not_panic() {
        let result = parse_mime(b"not a valid email at all", "acc1", "INBOX", 1);
        // mail-parser may partially parse, just ensure no panic
        let _ = result;
    }
}
