use serde::{Deserialize, Serialize};

/// 案件ノート。正本は DB 側（PIGEON-CONTEXT.md はディレクトリ連携時のミラー）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectNote {
    pub project_id: String,
    /// 「ノート」タブ: ユーザー手書き（Markdown/GFM）
    pub user_md: String,
    /// 「AI要約」タブ: AI が生成した下書き。ユーザー編集可
    pub ai_md: Option<String>,
    /// ユーザーが ai_md を手修正したか（再生成時の確認ダイアログ判定に使う）
    pub ai_edited: bool,
    pub ai_generated_at: Option<String>,
    pub updated_at: Option<String>,
}

/// AI要約の再生成履歴。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiHistoryEntry {
    pub id: String,
    pub project_id: String,
    pub ai_md: String,
    pub replaced_at: String,
}
