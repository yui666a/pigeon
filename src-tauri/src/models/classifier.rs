use serde::{Deserialize, Serialize};

pub const CONFIDENCE_AUTO_ASSIGN: f64 = 0.7;
pub const CONFIDENCE_UNCERTAIN: f64 = 0.4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailSummary {
    pub subject: String,
    pub from_addr: String,
    pub date: String,
    pub body_preview: String,
}

impl MailSummary {
    pub fn from_mail(mail: &crate::models::mail::Mail) -> Self {
        let body_preview = mail
            .body_text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        Self {
            subject: mail.subject.clone(),
            from_addr: mail.from_addr.clone(),
            date: mail.date.clone(),
            body_preview,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub recent_subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionEntry {
    pub mail_subject: String,
    pub from_project: Option<String>,
    pub to_project: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ClassifyAction {
    #[serde(rename = "assign")]
    Assign { project_id: String },
    #[serde(rename = "create")]
    Create {
        project_name: String,
        description: String,
    },
    #[serde(rename = "unclassified")]
    Unclassified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResult {
    #[serde(flatten)]
    pub action: ClassifyAction,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResponse {
    pub mail_id: String,
    #[serde(flatten)]
    pub result: ClassifyResult,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::mail::Mail;

    fn make_mail(body_text: Option<&str>) -> Mail {
        Mail {
            id: "m1".into(),
            account_id: "acc1".into(),
            folder: "INBOX".into(),
            message_id: "<msg1@example.com>".into(),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "me@example.com".into(),
            cc_addr: None,
            subject: "Test Subject".into(),
            body_text: body_text.map(|s| s.to_string()),
            body_html: None,
            date: "2026-04-13T10:00:00".into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        }
    }

    #[test]
    fn test_from_mail_basic() {
        let mail = make_mail(Some("Hello, this is a short body."));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.subject, "Test Subject");
        assert_eq!(summary.from_addr, "sender@example.com");
        assert_eq!(summary.date, "2026-04-13T10:00:00");
        assert_eq!(summary.body_preview, "Hello, this is a short body.");
    }

    #[test]
    fn test_from_mail_truncates_body_at_300_chars() {
        let long_body = "a".repeat(500);
        let mail = make_mail(Some(&long_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.len(), 300);
    }

    #[test]
    fn test_from_mail_empty_body() {
        let mail = make_mail(None);
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview, "");
    }

    #[test]
    fn test_from_mail_multibyte_truncation() {
        let japanese_body = "あ".repeat(500);
        let mail = make_mail(Some(&japanese_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 300);
    }
}
