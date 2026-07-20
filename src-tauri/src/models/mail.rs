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
    pub is_read: bool,
    pub is_flagged: bool,
    pub fetched_at: String,
    /// uid がサーバー実 UID として確定しているか。サーバー取得行は true、
    /// 送信時にローカル保存する Sent 行（推定 uid）は false。
    /// Sent 同期の watermark 計算（確定行のみの max uid）に使う。
    pub uid_confirmed: bool,
    /// 以下2つは `mails` テーブルのカラムではなく、`mail_project_assignments`
    /// から JOIN して載せる注釈。割り当てのないメールでは `None` になる。
    /// 確信度が中程度（`CONFIDENCE_UNCERTAIN`〜`CONFIDENCE_AUTO_ASSIGN`）の
    /// AI 分類にユーザー確認を促すため、UI まで運ぶ
    /// （設計: docs/design/2026-04-13-phase2-ai-classification-design.md）。
    #[serde(default)]
    pub assigned_by: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// アカウント内の未読件数の集計（folder='INBOX' のみ対象）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnreadCounts {
    /// project_id → 未読件数
    pub by_project: std::collections::HashMap<String, u32>,
    /// 未分類メールの未読件数
    pub unclassified: u32,
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
    /// 集約表示（サブツリー展開）でのみ埋まる。メンバーメールの直接所属案件のうち、
    /// 選択ノード自身を除いたものの集合。選択ノード直属のみのスレッドは空。
    pub projects: Vec<ThreadProjectRef>,
}

/// スレッド一覧の1ページ分。
///
/// 一覧取得は必ず上限を持つ（ADR 0006 決定5）。切り出しの単位は「メール」ではなく
/// 「スレッド」——メール単位で LIMIT すると、同じスレッドの一部のメールだけが
/// 窓に入り、mail_count や参加者一覧が実データと食い違ったスレッドが UI に出る。
///
/// `has_more` は「この後ろにまだスレッドがあるか」。総件数は返さない
/// （COUNT(*) の全走査を毎回発生させないため）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadPage {
    pub threads: Vec<Thread>,
    pub has_more: bool,
}

/// 集約表示でスレッドに付ける「どの案件のメールか」の注釈。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadProjectRef {
    pub project_id: String,
    /// 選択ノードからの相対パス（例: 選択が「ツアー」なら "埼玉 > 音響"）。
    /// 階層内では同名案件が共存し得るため単一 name ではなくパスにする。
    pub display_path: String,
}
