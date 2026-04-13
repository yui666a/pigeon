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
