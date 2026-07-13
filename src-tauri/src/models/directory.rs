use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDirectory {
    pub id: String,
    pub project_id: String,
    pub path: String,
    pub is_primary: bool,
    pub status: String, // 'ok' | 'missing' | 'inaccessible' | 'error'
    pub last_scanned_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub id: String,
    pub directory_id: String,
    pub relative_path: String,
    pub size_bytes: i64,
    pub mtime: String,
    pub content_hash: Option<String>,
    pub content_kind: String, // 'none' | 'text' | 'pdf' | 'office' | 'other'
    pub extract_status: String, // 'ok' | 'skipped_too_large' | 'unsupported' | 'error'
    pub indexed_at: String,
}

/// スキャン結果の1ファイル分（DBに入る前の形）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFileEntry {
    pub relative_path: String,
    pub size_bytes: i64,
    pub mtime: String,
    pub content_hash: Option<String>,
    pub content_kind: String,
    pub extract_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudRule {
    pub id: String,
    pub directory_id: String,
    pub scope: String, // 'directory' | 'file'
    pub relative_path: String,
    pub allow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_id: String,
    pub cached_context: Option<String>,
    pub context_hash: Option<String>,
    pub inventory_hash: Option<String>,
    pub allow_cloud_context: bool,
    pub generated_at: Option<String>,
}
