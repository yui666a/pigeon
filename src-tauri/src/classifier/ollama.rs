use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::classifier::{prompt, LlmClassifier};
use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyResult, CorrectionEntry, MailSummary, ProjectSummary,
};

pub struct OllamaClassifier {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaClassifier {
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client");
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            client,
        }
    }

    fn extract_json(content: &str) -> Option<&str> {
        let start = content.find('{')?;
        let end = content.rfind('}')?;
        if start <= end {
            Some(&content[start..=end])
        } else {
            None
        }
    }

    pub fn parse_response(content: &str) -> Result<ClassifyResult, AppError> {
        let json_str = Self::extract_json(content).ok_or_else(|| {
            AppError::InvalidLlmResponse(format!("No JSON object found in response: {}", content))
        })?;

        serde_json::from_str::<ClassifyResult>(json_str).map_err(|e| {
            AppError::InvalidLlmResponse(format!(
                "Failed to parse ClassifyResult from '{}': {}",
                json_str, e
            ))
        })
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponseMessage {
    content: String,
}

#[async_trait]
impl LlmClassifier for OllamaClassifier {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError> {
        let user_prompt = prompt::build_user_prompt(mail, projects, corrections);

        let request_body = OllamaChatRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system".to_string(),
                    content: prompt::SYSTEM_PROMPT.to_string(),
                },
                OllamaMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                },
            ],
            stream: false,
        };

        let url = format!("{}/api/chat", self.endpoint);
        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::OllamaConnection(format!(
                "Ollama returned status {}",
                response.status()
            )));
        }

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| AppError::InvalidLlmResponse(e.to_string()))?;

        let content = &chat_response.message.content;

        match Self::parse_response(content) {
            Ok(result) => Ok(result),
            Err(_) => Ok(ClassifyResult {
                action: ClassifyAction::Unclassified,
                confidence: 0.0,
                reason: format!(
                    "LLMの応答を解析できませんでした。生の応答: {}",
                    &content[..content.len().min(100)]
                ),
            }),
        }
    }

    async fn health_check(&self) -> Result<(), AppError> {
        let url = format!("{}/api/tags", self.endpoint);
        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::OllamaConnection(format!(
                "Health check failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json() {
        let input = r#"{"action": "assign", "project_id": "p1", "confidence": 0.9, "reason": "test"}"#;
        let result = OllamaClassifier::extract_json(input);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), input);
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let input = r#"Sure, here is the JSON: {"action": "unclassified", "confidence": 0.2, "reason": "unclear"} Hope that helps!"#;
        let result = OllamaClassifier::extract_json(input);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with('{'));
        assert!(result.unwrap().ends_with('}'));
    }

    #[test]
    fn test_extract_json_no_json() {
        let input = "This response has no JSON at all.";
        let result = OllamaClassifier::extract_json(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_response_assign() {
        let content = r#"{"action": "assign", "project_id": "proj-123", "confidence": 0.85, "reason": "件名とプロジェクトの関連性が高い"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
        if let ClassifyAction::Assign { project_id } = result.action {
            assert_eq!(project_id, "proj-123");
        }
        assert!((result.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_response_create() {
        let content = r#"{"action": "create", "project_name": "新規プロジェクト", "description": "新しい案件", "confidence": 0.75, "reason": "既存プロジェクトとの一致なし"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Create { .. }));
        if let ClassifyAction::Create {
            project_name,
            description,
        } = result.action
        {
            assert_eq!(project_name, "新規プロジェクト");
            assert_eq!(description, "新しい案件");
        }
        assert!((result.confidence - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_response_unclassified() {
        let content = r#"{"action": "unclassified", "confidence": 0.2, "reason": "内容が曖昧すぎる"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Unclassified));
        assert!((result.confidence - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_response_with_surrounding_text() {
        let content = r#"I analyzed the email and determined the following result:
{"action": "assign", "project_id": "proj-abc", "confidence": 0.9, "reason": "プロジェクトに一致"}
Please let me know if you need anything else."#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
        if let ClassifyAction::Assign { project_id } = result.action {
            assert_eq!(project_id, "proj-abc");
        }
    }

    #[test]
    fn test_parse_response_invalid() {
        let content = "This is not valid JSON at all, just plain text.";
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_json_empty_string() {
        assert!(OllamaClassifier::extract_json("").is_none());
    }

    #[test]
    fn test_extract_json_only_open_brace() {
        assert!(OllamaClassifier::extract_json("{").is_none());
    }

    #[test]
    fn test_extract_json_only_close_brace() {
        assert!(OllamaClassifier::extract_json("}").is_none());
    }

    #[test]
    fn test_extract_json_nested_braces() {
        let input = r#"{"outer": {"inner": "value"}}"#;
        let result = OllamaClassifier::extract_json(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_parse_response_missing_confidence() {
        let content = r#"{"action": "unclassified", "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_missing_reason() {
        let content = r#"{"action": "unclassified", "confidence": 0.5}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_unknown_action() {
        let content = r#"{"action": "delete", "confidence": 0.5, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_assign_missing_project_id() {
        let content = r#"{"action": "assign", "confidence": 0.9, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_create_missing_fields() {
        let content = r#"{"action": "create", "confidence": 0.7, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_confidence_boundary_values() {
        let content = r#"{"action": "unclassified", "confidence": 0.0, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);

        let content = r#"{"action": "unclassified", "confidence": 1.0, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }
}
