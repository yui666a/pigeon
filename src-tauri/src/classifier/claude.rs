use async_trait::async_trait;
use serde::Serialize;

use crate::classifier::anthropic_common::{
    extract_text, user_messages, MessageParam, MessagesResponse,
};
use crate::classifier::{build_http_client, LlmClassifier, TextGenerator};
use crate::error::AppError;

const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;

pub struct ClaudeClassifier {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeClassifier {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self, AppError> {
        let client = build_http_client()?;
        Ok(Self {
            api_key: api_key.into(),
            model: model.into(),
            client,
        })
    }

    fn build_request(&self, system_prompt: &str, user_prompt: &str) -> MessagesRequest {
        MessagesRequest {
            model: self.model.clone(),
            max_tokens: MAX_TOKENS,
            system: system_prompt.to_string(),
            messages: user_messages(user_prompt),
        }
    }

    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let body = self.build_request(system_prompt, user_prompt);
        let response = self
            .client
            .post(ANTHROPIC_MESSAGES_URL)
            .header("content-type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::Classifier(format!(
                "Anthropic API returned status {}",
                response.status()
            )));
        }

        let parsed: MessagesResponse = response
            .json()
            .await
            .map_err(|e| AppError::InvalidLlmResponse(e.to_string()))?;
        extract_text(&parsed)
    }
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<MessageParam>,
}

#[async_trait]
impl TextGenerator for ClaudeClassifier {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError> {
        self.chat(system_prompt, user_prompt).await
    }
}

/// classify は trait のデフォルト実装（generate_text 経由）を使う。
#[async_trait]
impl LlmClassifier for ClaudeClassifier {
    fn model_id(&self) -> String {
        format!("claude:{}", self.model)
    }

    async fn health_check(&self) -> Result<(), AppError> {
        let response = self
            .client
            .get(ANTHROPIC_MODELS_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Classifier(format!(
                "Anthropic health check failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_shape() {
        let c = ClaudeClassifier::new("sk-test", "claude-haiku-4-5").unwrap();
        let req = c.build_request("sys", "usr");
        assert_eq!(req.model, "claude-haiku-4-5");
        assert_eq!(req.max_tokens, MAX_TOKENS);
        assert_eq!(req.system, "sys");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content, "usr");
    }
}
