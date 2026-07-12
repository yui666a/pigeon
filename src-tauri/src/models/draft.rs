use serde::{Deserialize, Serialize};

/// ローカル下書き（v1: IMAP Draftsフォルダとの同期は将来）。
/// 詳細: docs/superpowers/specs/2026-07-12-draft-save-design.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub account_id: String,
    /// ComposeModal の入力欄と同じくカンマ区切り文字列で保持する
    pub to_addr: String,
    pub cc_addr: String,
    pub bcc_addr: String,
    pub subject: String,
    pub body_text: String,
    /// 返信元メールのローカルID（SendMailRequest.reply_to_mail_id と同じ意味）
    pub in_reply_to: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
