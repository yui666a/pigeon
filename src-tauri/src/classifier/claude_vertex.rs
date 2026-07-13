use async_trait::async_trait;
use gcp_auth::{CustomServiceAccount, TokenProvider};
use serde::{Deserialize, Serialize};

use crate::classifier::{build_http_client, LlmClassifier, TextGenerator};
use crate::error::AppError;

/// Vertex 上では anthropic_version はヘッダではなくボディに入れ、値はこの固定文字列。
const VERTEX_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";
const GCP_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const MAX_TOKENS: u32 = 1024;

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
        let service_account = CustomServiceAccount::from_json(sa_json)
            .map_err(|e| AppError::MissingApiKey(format!("claude_vertex (invalid SA JSON: {e})")))?;
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
    ///
    /// `global` はホスト名が特殊で、`global-aiplatform...` ではなく
    /// `aiplatform.googleapis.com`（プレフィックス無し）になる。それ以外の
    /// リージョンは `{location}-aiplatform.googleapis.com`。
    fn endpoint_url(location: &str, project_id: &str, model: &str) -> String {
        let host = if location == "global" {
            "aiplatform.googleapis.com".to_string()
        } else {
            format!("{location}-aiplatform.googleapis.com")
        };
        format!(
            "https://{host}/v1/projects/{project_id}/locations/{location}/publishers/anthropic/models/{model}:rawPredict"
        )
    }

    /// Vertex 用リクエストボディを組み立てる。`model` は含めず `anthropic_version` を入れる。
    fn build_request(system_prompt: &str, user_prompt: &str) -> VertexMessagesRequest {
        VertexMessagesRequest {
            anthropic_version: VERTEX_ANTHROPIC_VERSION.to_string(),
            max_tokens: MAX_TOKENS,
            system: system_prompt.to_string(),
            messages: vec![MessageParam {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }],
        }
    }

    /// レスポンス JSON から最初の text ブロックを取り出す（Messages API と同形）。
    fn extract_text(resp: &MessagesResponse) -> Result<String, AppError> {
        resp.content
            .iter()
            .find_map(|b| {
                if b.block_type == "text" {
                    b.text.clone()
                } else {
                    None
                }
            })
            .ok_or_else(|| AppError::InvalidLlmResponse("no text block in response".to_string()))
    }

    /// SA からアクセストークンを取得する（クレート側でキャッシュ・失効管理される）。
    async fn access_token(&self) -> Result<String, AppError> {
        let token = self
            .service_account
            .token(&[GCP_SCOPE])
            .await
            .map_err(|e| AppError::HttpRequest(format!("Vertex token error: {e}")))?;
        Ok(token.as_str().to_string())
    }

    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let url = Self::endpoint_url(&self.location, &self.project_id, &self.model);
        let token = self.access_token().await?;
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
        Self::extract_text(&parsed)
    }
}

#[derive(Debug, Serialize)]
struct VertexMessagesRequest {
    anthropic_version: String,
    max_tokens: u32,
    system: String,
    messages: Vec<MessageParam>,
}

#[derive(Debug, Serialize)]
struct MessageParam {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
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
    /// 専用の軽量エンドポイントが無いため、最小のダミーメッセージを rawPredict に投げて
    /// 疎通・認証・権限をまとめて検証する。
    async fn health_check(&self) -> Result<(), AppError> {
        let url = Self::endpoint_url(&self.location, &self.project_id, &self.model);
        let token = self.access_token().await?;
        let body = VertexMessagesRequest {
            anthropic_version: VERTEX_ANTHROPIC_VERSION.to_string(),
            max_tokens: 1,
            system: String::new(),
            messages: vec![MessageParam {
                role: "user".to_string(),
                content: "ping".to_string(),
            }],
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

    #[test]
    fn test_endpoint_url_shape() {
        let url = ClaudeVertexClassifier::endpoint_url(
            "us-east5",
            "pigeon-mail-xxxxxx",
            "claude-haiku-4-5@20251001",
        );
        assert_eq!(
            url,
            "https://us-east5-aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/us-east5/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict"
        );
    }

    #[test]
    fn test_endpoint_url_global_has_no_region_prefix() {
        // global はホスト名にリージョン接頭辞が付かない。
        let url = ClaudeVertexClassifier::endpoint_url(
            "global",
            "pigeon-mail-xxxxxx",
            "claude-haiku-4-5@20251001",
        );
        assert_eq!(
            url,
            "https://aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/global/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict"
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
    fn test_extract_text_finds_text_block() {
        let resp = MessagesResponse {
            content: vec![ContentBlock {
                block_type: "text".to_string(),
                text: Some("{\"action\":\"unclassified\"}".to_string()),
            }],
        };
        assert_eq!(
            ClaudeVertexClassifier::extract_text(&resp).unwrap(),
            "{\"action\":\"unclassified\"}"
        );
    }

    #[test]
    fn test_extract_text_no_text_block_errs() {
        let resp = MessagesResponse {
            content: vec![ContentBlock {
                block_type: "tool_use".to_string(),
                text: None,
            }],
        };
        assert!(ClaudeVertexClassifier::extract_text(&resp).is_err());
    }

    #[test]
    fn test_response_deserializes_from_api_json() {
        let json = r#"{"content":[{"type":"text","text":"hello"}]}"#;
        let resp: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(ClaudeVertexClassifier::extract_text(&resp).unwrap(), "hello");
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
