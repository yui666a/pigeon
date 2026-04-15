use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mail {
    pub id: String,
    pub account_id: String,
    pub folder: String,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub from_addr: String,
    pub to_addr: String,
    pub cc_addr: Option<String>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub date: String,
    pub has_attachments: bool,
    pub raw_size: Option<i64>,
    pub uid: u32,
    pub flags: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub mail: Mail,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub thread_id: String,
    pub subject: String,
    pub last_date: String,
    pub mail_count: usize,
    pub from_addrs: Vec<String>,
    pub mails: Vec<Mail>,
}
