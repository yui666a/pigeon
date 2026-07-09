pub mod ollama;
pub mod prompt;

use crate::error::AppError;
use crate::models::classifier::{ClassifyResult, CorrectionEntry, MailSummary, ProjectSummary};
use async_trait::async_trait;

#[async_trait]
pub trait LlmClassifier: Send + Sync {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError>;

    async fn health_check(&self) -> Result<(), AppError>;
}

/// 汎用テキスト生成（ダイジェスト生成等に使用）。LlmClassifier と同じプロバイダが実装する。
#[async_trait]
pub trait TextGenerator: Send + Sync {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError>;
}
