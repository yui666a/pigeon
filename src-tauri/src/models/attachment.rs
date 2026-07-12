use serde::{Deserialize, Serialize};

/// メールの添付ファイル。実体はローカルキャッシュ（file_path）に保存される。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub mail_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: Option<i64>,
    /// キャッシュファイルの絶対パス。ファイルが消えている場合はキャッシュミス扱い
    pub file_path: Option<String>,
}
