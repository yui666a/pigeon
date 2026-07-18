use serde::{Deserialize, Serialize};

/// 分類プロンプトに載せる本文プレビューの最大文字数。
pub const BODY_PREVIEW_CHARS: usize = 1000;

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
            .take(BODY_PREVIEW_CHARS)
            .collect();
        Self {
            subject: mail.subject.clone(),
            from_addr: mail.from_addr.clone(),
            date: mail.date.clone(),
            body_preview,
        }
    }
}

/// 複数メールから提案された新規案件の名前・説明。
/// LLM 提案をフロントのフォーム初期値として返すための型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSuggestion {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    /// ルート→自ノードのフルパス（例:「ツアー > 埼玉 > 音響」）。フラット案件は name と同じ。
    pub path: String,
    pub description: Option<String>,
    pub recent_subjects: Vec<String>,
    pub top_senders: Vec<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionEntry {
    pub mail_subject: String,
    /// 訂正時点のパススナップショット。None = 未分類からの移動
    pub from_path: Option<String>,
    pub to_path: String,
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
        /// 既存案件配下に子案件として作成する提案。存在しない/別アカウントの
        /// 場合は apply_result 内でルート作成（None）に落とす（設計書 §6）。
        #[serde(default)]
        parent_project_id: Option<String>,
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

/// `classify_batch` の戻り値。1 invoke で「次の停止点（create 提案）or 完了/中断」
/// まで進んだ結果を表す（設計: 2026-07-13-classify-batch-backend-design.md）。
///
/// `done` は処理済み件数、`total` はバッチ開始時のキュー長（再開しても不変）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClassifyBatchOutcome {
    /// キューを最後まで処理した
    Completed { done: usize, total: usize },
    /// 新規案件提案（create）で停止。承認/却下後に再 invoke で続きから再開する
    Paused {
        proposal: ClassifyResponse,
        done: usize,
        total: usize,
    },
    /// `cancel_classification` により中断（バッチは破棄済み）
    Cancelled { done: usize, total: usize },
    /// 同一アカウントのバッチが実行中のため何もしなかった
    AlreadyRunning,
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
            is_read: false,
            is_flagged: false,
            fetched_at: "2026-04-13T00:00:00".into(),
            uid_confirmed: true,
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
    fn test_from_mail_truncates_body_at_1000_chars() {
        let long_body = "a".repeat(1500);
        let mail = make_mail(Some(&long_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 1000);
    }

    #[test]
    fn test_from_mail_body_under_limit_kept_whole() {
        let body = "a".repeat(700);
        let mail = make_mail(Some(&body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 700);
    }

    #[test]
    fn test_from_mail_empty_body() {
        let mail = make_mail(None);
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview, "");
    }

    #[test]
    fn test_from_mail_multibyte_truncation() {
        let japanese_body = "あ".repeat(1500);
        let mail = make_mail(Some(&japanese_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 1000);
    }

    // --- ClassifyAction::Create: parent_project_id のパース ---

    #[test]
    fn test_create_action_parses_parent_project_id() {
        let json = r#"{"action":"create","project_name":"音響","description":"d","parent_project_id":"root","confidence":0.8,"reason":"r"}"#;
        let result: ClassifyResult = serde_json::from_str(json).unwrap();
        match result.action {
            ClassifyAction::Create {
                parent_project_id, ..
            } => {
                assert_eq!(parent_project_id.as_deref(), Some("root"));
            }
            _ => panic!("expected create"),
        }
    }

    #[test]
    fn test_create_action_without_parent_still_parses() {
        // 旧形式の応答（parent なし）も壊れない
        let json = r#"{"action":"create","project_name":"音響","description":"d","confidence":0.8,"reason":"r"}"#;
        let result: ClassifyResult = serde_json::from_str(json).unwrap();
        match result.action {
            ClassifyAction::Create {
                parent_project_id, ..
            } => assert!(parent_project_id.is_none()),
            _ => panic!("expected create"),
        }
    }
}
