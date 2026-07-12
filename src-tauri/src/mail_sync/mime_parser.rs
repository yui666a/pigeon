use mail_parser::{MessageParser, MimeHeaders};
use uuid::Uuid;

use crate::models::mail::Mail;

/// MIMEから抽出した添付ファイル（DB登録前の生データ）
#[derive(Debug, Clone)]
pub struct ExtractedAttachment {
    pub filename: Option<String>,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// 元メールのバイト列から添付ファイルを抽出する純関数。
/// mail-parser が添付と判定したパート（本文以外のパート）を返す。
pub fn extract_attachments(raw: &[u8]) -> Vec<ExtractedAttachment> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    message
        .attachments()
        .map(|part| {
            let mime_type = part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{}", ct.ctype(), sub),
                    None => ct.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".to_string());
            ExtractedAttachment {
                filename: part.attachment_name().map(|s| s.to_string()),
                mime_type,
                data: part.contents().to_vec(),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn parse_mime(
    raw: &[u8],
    account_id: &str,
    folder: &str,
    uid: u32,
    is_read: bool,
    is_flagged: bool,
    flags: Option<String>,
) -> Option<Mail> {
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
        flags,
        is_read,
        is_flagged,
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
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1, false, false, None).unwrap();
        assert_eq!(mail.subject, "Test Email");
        assert_eq!(mail.from_addr, "sender@example.com");
        assert_eq!(mail.to_addr, "recipient@example.com");
        assert_eq!(mail.message_id, "<test123@example.com>");
        assert!(mail.in_reply_to.is_none());
        assert!(mail.body_text.unwrap().contains("Hello"));
    }

    #[test]
    fn test_parse_reply_email() {
        let mail = parse_mime(REPLY_EMAIL, "acc1", "INBOX", 2, false, false, None).unwrap();
        assert_eq!(mail.subject, "Re: Test Email");
        assert!(mail.in_reply_to.is_some());
        assert!(mail.references.is_some());
    }

    #[test]
    fn test_parse_invalid_does_not_panic() {
        let result = parse_mime(b"not a valid email at all", "acc1", "INBOX", 1, false, false, None);
        // mail-parser may partially parse, just ensure no panic
        let _ = result;
    }

    const EMAIL_WITH_CC: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Cc: cc1@example.com, cc2@example.com\r\n\
        Subject: CC Test\r\n\
        Message-ID: <cc-test@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Body with CC.";

    const EMAIL_NO_SUBJECT: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Message-ID: <nosub@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Body without subject.";

    const EMAIL_WITH_DISPLAY_NAME: &[u8] = b"From: Alice Smith <alice@example.com>\r\n\
        To: Bob Jones <bob@example.com>\r\n\
        Subject: Display Name Test\r\n\
        Message-ID: <display@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Hello Bob.";

    const EMAIL_WITH_REFERENCES_CHAIN: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: Re: Re: Chain\r\n\
        Message-ID: <chain3@example.com>\r\n\
        In-Reply-To: <chain2@example.com>\r\n\
        References: <chain1@example.com> <chain2@example.com>\r\n\
        Date: Mon, 13 Apr 2026 12:00:00 +0900\r\n\
        \r\n\
        Third reply.";

    #[test]
    fn test_parse_email_with_cc() {
        let mail = parse_mime(EMAIL_WITH_CC, "acc1", "INBOX", 3, false, false, None).unwrap();
        assert!(mail.cc_addr.is_some());
        let cc = mail.cc_addr.unwrap();
        assert!(cc.contains("cc1@example.com"));
        assert!(cc.contains("cc2@example.com"));
    }

    #[test]
    fn test_parse_email_no_subject_defaults() {
        let mail = parse_mime(EMAIL_NO_SUBJECT, "acc1", "INBOX", 4, false, false, None).unwrap();
        assert_eq!(mail.subject, "(no subject)");
    }

    #[test]
    fn test_parse_email_with_display_name() {
        let mail = parse_mime(EMAIL_WITH_DISPLAY_NAME, "acc1", "INBOX", 5, false, false, None).unwrap();
        assert!(mail.from_addr.contains("Alice Smith"));
        assert!(mail.from_addr.contains("alice@example.com"));
    }

    #[test]
    fn test_parse_email_with_references_chain() {
        let mail = parse_mime(EMAIL_WITH_REFERENCES_CHAIN, "acc1", "INBOX", 6, false, false, None).unwrap();
        assert_eq!(mail.in_reply_to, Some("<chain2@example.com>".to_string()));
        let refs = mail.references.unwrap();
        assert!(refs.contains("<chain1@example.com>"));
        assert!(refs.contains("<chain2@example.com>"));
    }

    #[test]
    fn test_parse_email_sets_account_and_folder() {
        let mail = parse_mime(SIMPLE_EMAIL, "my-account", "Sent", 10, false, false, None).unwrap();
        assert_eq!(mail.account_id, "my-account");
        assert_eq!(mail.folder, "Sent");
        assert_eq!(mail.uid, 10);
    }

    #[test]
    fn test_parse_email_propagates_read_state_and_flags() {
        let mail = parse_mime(
            SIMPLE_EMAIL,
            "acc1",
            "INBOX",
            1,
            true,
            true,
            Some("\\Seen \\Flagged".into()),
        )
        .unwrap();
        assert!(mail.is_read);
        assert!(mail.is_flagged);
        assert_eq!(mail.flags, Some("\\Seen \\Flagged".to_string()));

        let unread = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 2, false, false, None).unwrap();
        assert!(!unread.is_read);
        assert!(!unread.is_flagged);
        assert!(unread.flags.is_none());
    }

    #[test]
    fn test_parse_email_no_attachments() {
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1, false, false, None).unwrap();
        assert!(!mail.has_attachments);
    }

    #[test]
    fn test_parse_email_raw_size() {
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1, false, false, None).unwrap();
        assert_eq!(mail.raw_size, Some(SIMPLE_EMAIL.len() as i64));
    }

    #[test]
    fn test_parse_empty_bytes() {
        let result = parse_mime(b"", "acc1", "INBOX", 1, false, false, None);
        let _ = result;
    }

    const MULTIPART_EMAIL_WITH_ATTACHMENTS: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: With Attachments\r\n\
        Message-ID: <att@example.com>\r\n\
        Date: Sun, 12 Jul 2026 10:00:00 +0900\r\n\
        MIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"BOUNDARY\"\r\n\
        \r\n\
        --BOUNDARY\r\n\
        Content-Type: text/plain\r\n\
        \r\n\
        Please find attached.\r\n\
        --BOUNDARY\r\n\
        Content-Type: application/pdf; name=\"report.pdf\"\r\n\
        Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
        Content-Transfer-Encoding: base64\r\n\
        \r\n\
        JVBERi0xLjQK\r\n\
        --BOUNDARY\r\n\
        Content-Type: image/png; name=\"pic.png\"\r\n\
        Content-Disposition: attachment; filename=\"pic.png\"\r\n\
        Content-Transfer-Encoding: base64\r\n\
        \r\n\
        iVBORw0KGgo=\r\n\
        --BOUNDARY--\r\n";

    const ATTACHMENT_WITHOUT_FILENAME: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: Nameless\r\n\
        Message-ID: <nameless@example.com>\r\n\
        Date: Sun, 12 Jul 2026 10:00:00 +0900\r\n\
        MIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"BOUNDARY\"\r\n\
        \r\n\
        --BOUNDARY\r\n\
        Content-Type: text/plain\r\n\
        \r\n\
        Body.\r\n\
        --BOUNDARY\r\n\
        Content-Type: application/octet-stream\r\n\
        Content-Disposition: attachment\r\n\
        Content-Transfer-Encoding: base64\r\n\
        \r\n\
        AAECAw==\r\n\
        --BOUNDARY--\r\n";

    #[test]
    fn test_extract_attachments_multipart() {
        let attachments = extract_attachments(MULTIPART_EMAIL_WITH_ATTACHMENTS);
        assert_eq!(attachments.len(), 2);

        assert_eq!(attachments[0].filename.as_deref(), Some("report.pdf"));
        assert_eq!(attachments[0].mime_type, "application/pdf");
        assert_eq!(attachments[0].data, b"%PDF-1.4\n");

        assert_eq!(attachments[1].filename.as_deref(), Some("pic.png"));
        assert_eq!(attachments[1].mime_type, "image/png");
        assert_eq!(attachments[1].data, b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn test_extract_attachments_marks_has_attachments() {
        // 同期時の has_attachments フラグと抽出結果が整合すること
        let mail = parse_mime(MULTIPART_EMAIL_WITH_ATTACHMENTS, "acc1", "INBOX", 1, false, false, None).unwrap();
        assert!(mail.has_attachments);
    }

    #[test]
    fn test_extract_attachments_none_for_plain_email() {
        assert!(extract_attachments(SIMPLE_EMAIL).is_empty());
    }

    #[test]
    fn test_extract_attachments_without_filename() {
        let attachments = extract_attachments(ATTACHMENT_WITHOUT_FILENAME);
        assert_eq!(attachments.len(), 1);
        assert!(attachments[0].filename.is_none());
        assert_eq!(attachments[0].mime_type, "application/octet-stream");
        assert_eq!(attachments[0].data, [0u8, 1, 2, 3]);
    }

    #[test]
    fn test_extract_attachments_invalid_bytes() {
        assert!(extract_attachments(b"").is_empty());
        assert!(extract_attachments(b"garbage").is_empty());
    }
}
