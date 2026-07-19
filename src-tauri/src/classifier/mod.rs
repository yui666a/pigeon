pub mod anthropic_common;
pub mod claude;
pub mod claude_vertex;
pub mod factory;
pub mod gemini_vertex;
pub mod ollama;
pub mod parse;
pub mod prompt;
pub mod service;
pub mod vertex_common;

use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyResult, CorrectionEntry, MailSummary, ProjectSummary,
};
use async_trait::async_trait;
use std::time::Duration;

/// 全プロバイダ共通の HTTP クライアント（30秒タイムアウト）を生成する。
pub(crate) fn build_http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::HttpRequest(e.to_string()))
}

/// パース失敗時のフォールバック結果を組み立てる。
/// 生応答のプレビューは char 境界で切り詰める（バイト境界だとマルチバイト応答でパニックする）。
fn fallback_unclassified(content: &str) -> ClassifyResult {
    let preview: String = content.chars().take(100).collect();
    ClassifyResult {
        action: ClassifyAction::Unclassified,
        confidence: 0.0,
        reason: format!("LLMの応答を解析できませんでした。生の応答: {preview}"),
    }
}

/// 汎用テキスト生成（ダイジェスト生成等）。全プロバイダが実装する。
#[async_trait]
pub trait TextGenerator: Send + Sync {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError>;
}

#[async_trait]
pub trait LlmClassifier: TextGenerator + Send + Sync {
    /// 判断の記録に残す "provider:model" 形式の識別子。モデルを変えると
    /// 確信度の性質も変わるため、キャリブレーション分析の軸として使う。
    /// 秘密情報（APIキー等）は含めない。
    fn model_id(&self) -> String;

    /// メールを分類する。プロンプト組み立て → LLM 呼び出し → パースの流れは
    /// 全プロバイダ共通なので、デフォルト実装として提供する。
    /// 応答をパースできない場合は Unclassified にフォールバックする。
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError> {
        let user_prompt = prompt::build_user_prompt(mail, projects, corrections);
        let content = self
            .generate_text(prompt::SYSTEM_PROMPT, &user_prompt)
            .await?;
        Ok(parse::parse_classify_result(&content)
            .unwrap_or_else(|_| fallback_unclassified(&content)))
    }

    async fn health_check(&self) -> Result<(), AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 固定応答を返すモック。呼び出し時のプロンプトも記録する。
    struct MockLlm {
        /// None ならエラーを返す。
        response: Option<String>,
        captured: Mutex<Option<(String, String)>>,
    }

    impl MockLlm {
        fn with_response(response: &str) -> Self {
            Self {
                response: Some(response.to_string()),
                captured: Mutex::new(None),
            }
        }

        fn failing() -> Self {
            Self {
                response: None,
                captured: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl TextGenerator for MockLlm {
        async fn generate_text(
            &self,
            system_prompt: &str,
            user_prompt: &str,
        ) -> Result<String, AppError> {
            *self.captured.lock().unwrap() =
                Some((system_prompt.to_string(), user_prompt.to_string()));
            self.response
                .clone()
                .ok_or_else(|| AppError::Classifier("mock failure".to_string()))
        }
    }

    #[async_trait]
    impl LlmClassifier for MockLlm {
        fn model_id(&self) -> String {
            "stub:test".into()
        }

        async fn health_check(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn make_mail() -> MailSummary {
        MailSummary {
            subject: "見積もりの件".to_string(),
            from_addr: "sender@example.com".to_string(),
            date: "2026-07-13T10:00:00".to_string(),
            body_preview: "本文プレビュー".to_string(),
        }
    }

    #[tokio::test]
    async fn test_default_classify_parses_valid_response() {
        let llm = MockLlm::with_response(
            r#"{"action": "assign", "project_id": "proj-1", "confidence": 0.9, "reason": "r"}"#,
        );
        let result = llm.classify(&make_mail(), &[], &[]).await.unwrap();
        assert!(matches!(
            result.action,
            ClassifyAction::Assign { ref project_id } if project_id == "proj-1"
        ));
        assert!((result.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_default_classify_sends_system_and_user_prompt() {
        let llm = MockLlm::with_response(
            r#"{"action": "unclassified", "confidence": 0.1, "reason": "r"}"#,
        );
        llm.classify(&make_mail(), &[], &[]).await.unwrap();
        let captured = llm.captured.lock().unwrap();
        let (system, user) = captured.as_ref().unwrap();
        assert_eq!(system, prompt::SYSTEM_PROMPT);
        assert!(user.contains("見積もりの件"));
        assert!(user.contains("sender@example.com"));
    }

    #[tokio::test]
    async fn test_default_classify_falls_back_on_unparseable_response() {
        let llm = MockLlm::with_response("これはJSONではありません");
        let result = llm.classify(&make_mail(), &[], &[]).await.unwrap();
        assert!(matches!(result.action, ClassifyAction::Unclassified));
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
        assert!(result.reason.contains("解析できませんでした"));
        assert!(result.reason.contains("これはJSONではありません"));
    }

    #[tokio::test]
    async fn test_default_classify_multibyte_fallback_does_not_panic() {
        // 従来の &content[..100] はバイト境界スライスのため「あ」(3バイト) の
        // 連続でパニックしていた。char 境界で切り詰めることを確認する。
        let long_multibyte = "あ".repeat(200);
        let llm = MockLlm::with_response(&long_multibyte);
        let result = llm.classify(&make_mail(), &[], &[]).await.unwrap();
        assert!(matches!(result.action, ClassifyAction::Unclassified));
        assert!(result.reason.contains(&"あ".repeat(100)));
        assert!(!result.reason.contains(&"あ".repeat(101)));
    }

    #[tokio::test]
    async fn test_default_classify_propagates_llm_error() {
        let llm = MockLlm::failing();
        let result = llm.classify(&make_mail(), &[], &[]).await;
        assert!(matches!(result, Err(AppError::Classifier(_))));
    }

    #[test]
    fn test_fallback_unclassified_keeps_short_response_whole() {
        let result = fallback_unclassified("short");
        assert!(matches!(result.action, ClassifyAction::Unclassified));
        assert!(result.reason.contains("short"));
    }

    #[test]
    fn test_build_http_client_succeeds() {
        assert!(build_http_client().is_ok());
    }
}
