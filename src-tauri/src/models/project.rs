use crate::models::directory::ProjectDirectory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub is_archived: bool,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 案件と、その主ディレクトリ（未紐付けなら None）を1件にまとめたもの。
///
/// 案件一覧の取得で案件ごとに `get_project_directory` を往復すると、案件数 N に
/// 対して IPC が N 回発生する（ADR 0006「本 ADR の射程外」で個別の実装改善と
/// 位置づけられた N+1）。LEFT JOIN 1 クエリ・1 往復で返すためにこの型を使う。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectWithDirectory {
    #[serde(flatten)]
    pub project: Project,
    pub directory: Option<ProjectDirectory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
}
