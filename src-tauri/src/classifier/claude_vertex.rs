use async_trait::async_trait;
use gcp_auth::CustomServiceAccount;
use serde::Serialize;

use crate::classifier::anthropic_common::{
    extract_text, user_messages, MessageParam, MessagesResponse,
};
use crate::classifier::vertex_common;
use crate::classifier::{build_http_client, LlmClassifier, TextGenerator};
use crate::error::AppError;

/// Vertex 上では anthropic_version はヘッダではなくボディに入れ、値はこの固定文字列。
const VERTEX_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";
const MAX_TOKENS: u32 = 1024;
const PUBLISHER: &str = "anthropic";
const METHOD: &str = "rawPredict";

/// Claude on Vertex AI（GCP Agent Platform）経由のクラシファイア。
/// Anthropic 直 API (`ClaudeClassifier`) との差分は、エンドポイント・認証（Bearer トークン）・
/// ボディの `anthropic_version`（`model` はボディに含めず URL パスで指定）のみ。
pub struct ClaudeVertexClassifier {
    service_account: CustomServiceAccount,
    project_id: String,
    location: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeVertexClassifier {
    pub fn new(
        sa_json: &str,
        project_id: impl Into<String>,
        location: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, AppError> {
        let service_account = vertex_common::parse_service_account(sa_json, "claude_vertex")?;
        let client = build_http_client()?;
        Ok(Self {
            service_account,
            project_id: project_id.into(),
            location: location.into(),
            model: model.into(),
            client,
        })
    }

    /// rawPredict エンドポイント URL を組み立てる。
    fn endpoint_url(&self) -> String {
        vertex_common::endpoint_url(
            &self.location,
            &self.project_id,
            PUBLISHER,
            &self.model,
            METHOD,
        )
    }

    /// Vertex 用リクエストボディを組み立てる。`model` は含めず `anthropic_version` を入れる。
    fn build_request(system_prompt: &str, user_prompt: &str) -> VertexMessagesRequest {
        VertexMessagesRequest {
            anthropic_version: VERTEX_ANTHROPIC_VERSION.to_string(),
            max_tokens: MAX_TOKENS,
            system: system_prompt.to_string(),
            messages: user_messages(user_prompt),
        }
    }

    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let url = self.endpoint_url();
        let token = vertex_common::access_token(&self.service_account).await?;
        let body = Self::build_request(system_prompt, user_prompt);

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::Classifier(format!(
                "Vertex AI returned status {}",
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
struct VertexMessagesRequest {
    anthropic_version: String,
    max_tokens: u32,
    system: String,
    messages: Vec<MessageParam>,
}

#[async_trait]
impl TextGenerator for ClaudeVertexClassifier {
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
impl LlmClassifier for ClaudeVertexClassifier {
    fn model_id(&self) -> String {
        format!("vertex:{}", self.model)
    }

    /// 専用の軽量エンドポイントが無いため、最小のダミーメッセージを rawPredict に投げて
    /// 疎通・認証・権限をまとめて検証する。
    async fn health_check(&self) -> Result<(), AppError> {
        let url = self.endpoint_url();
        let token = vertex_common::access_token(&self.service_account).await?;
        let body = VertexMessagesRequest {
            anthropic_version: VERTEX_ANTHROPIC_VERSION.to_string(),
            max_tokens: 1,
            system: String::new(),
            messages: user_messages("ping"),
        };
        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Classifier(format!(
                "Vertex AI health check failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_classifier(location: &str) -> ClaudeVertexClassifier {
        let sa_json = crate::test_helpers::test_sa_json();
        ClaudeVertexClassifier::new(
            sa_json,
            "pigeon-mail-xxxxxx",
            location,
            "claude-haiku-4-5@20251001",
        )
        .unwrap()
    }

    #[test]
    fn test_endpoint_url_uses_anthropic_raw_predict() {
        let c = make_classifier("us-east5");
        assert_eq!(
            c.endpoint_url(),
            "https://us-east5-aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/us-east5/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict"
        );
    }

    #[test]
    fn test_build_request_has_vertex_version_and_no_model() {
        let req = ClaudeVertexClassifier::build_request("sys", "usr");
        assert_eq!(req.anthropic_version, "vertex-2023-10-16");
        assert_eq!(req.max_tokens, MAX_TOKENS);
        assert_eq!(req.system, "sys");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content, "usr");
        // model はボディに含めない → シリアライズしても "model" キーが無い
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("model").is_none());
        assert_eq!(json["anthropic_version"], "vertex-2023-10-16");
    }

    #[test]
    fn test_new_rejects_invalid_sa_json() {
        let result = ClaudeVertexClassifier::new(
            "not-json",
            "proj",
            "us-east5",
            "claude-haiku-4-5@20251001",
        );
        assert!(matches!(result, Err(AppError::MissingApiKey(_))));
    }
}
