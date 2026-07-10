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
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;
        Ok(Self {
            endpoint: endpoint.into(),
            model: model.into(),
            client,
        })
    }

    /// /api/chat を呼び、応答テキストを返す（classify と TextGenerator の共通部）。
    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let request_body = OllamaChatRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                OllamaMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
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
        Ok(chat_response.message.content)
    }
}

#[async_trait]
impl crate::classifier::TextGenerator for OllamaClassifier {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError> {
        self.chat(system_prompt, user_prompt).await
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
        let content = self.chat(prompt::SYSTEM_PROMPT, &user_prompt).await?;

        match crate::classifier::parse::parse_classify_result(&content) {
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
