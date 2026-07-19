use async_trait::async_trait;
use gcp_auth::CustomServiceAccount;
use serde::{Deserialize, Serialize};

use crate::classifier::vertex_common;
use crate::classifier::{build_http_client, LlmClassifier, TextGenerator};
use crate::error::AppError;

// Gemini は思考トークン（thoughtSignature）を先に消費することがあるため、
// 分類 JSON を確実に出力できるよう十分な上限を取る。
const MAX_OUTPUT_TOKENS: u32 = 1024;
const PUBLISHER: &str = "google";
const METHOD: &str = "generateContent";

/// Gemini on Vertex AI（GCP Agent Platform）経由のクラシファイア。
/// 認証（SA JSON → Bearer トークン）は `ClaudeVertexClassifier` と共通だが、
/// エンドポイント（`publishers/google/.../:generateContent`）・リクエスト/レスポンス
/// の JSON 構造が Claude とは異なる。
pub struct GeminiVertexClassifier {
    service_account: CustomServiceAccount,
    project_id: String,
    location: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiVertexClassifier {
    pub fn new(
        sa_json: &str,
        project_id: impl Into<String>,
        location: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, AppError> {
        let service_account = vertex_common::parse_service_account(sa_json, "gemini_vertex")?;
        let client = build_http_client()?;
        Ok(Self {
            service_account,
            project_id: project_id.into(),
            location: location.into(),
            model: model.into(),
            client,
        })
    }

    /// generateContent エンドポイント URL を組み立てる。
    fn endpoint_url(&self) -> String {
        vertex_common::endpoint_url(
            &self.location,
            &self.project_id,
            PUBLISHER,
            &self.model,
            METHOD,
        )
    }

    /// Gemini 用リクエストボディを組み立てる。
    /// system は `systemInstruction`、user は `contents` に入れる（Claude とは別構造）。
    fn build_request(system_prompt: &str, user_prompt: &str) -> GenerateContentRequest {
        GenerateContentRequest {
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part {
                    text: Some(system_prompt.to_string()),
                }],
            }),
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part {
                    text: Some(user_prompt.to_string()),
                }],
            }],
            generation_config: GenerationConfig {
                max_output_tokens: MAX_OUTPUT_TOKENS,
            },
        }
    }

    /// レスポンス JSON から最初の候補のテキストを取り出す。
    /// Gemini は parts に text の無いブロック（thoughtSignature のみ等）を含むことがあるため、
    /// text を持つ part を連結する。
    fn extract_text(resp: &GenerateContentResponse) -> Result<String, AppError> {
        let candidate = resp
            .candidates
            .first()
            .ok_or_else(|| AppError::InvalidLlmResponse("no candidates in response".to_string()))?;
        let text: String = candidate
            .content
            .parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            return Err(AppError::InvalidLlmResponse(
                "no text part in candidate".to_string(),
            ));
        }
        Ok(text)
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
                "Vertex AI (Gemini) returned status {}",
                response.status()
            )));
        }

        let parsed: GenerateContentResponse = response
            .json()
            .await
            .map_err(|e| AppError::InvalidLlmResponse(e.to_string()))?;
        Self::extract_text(&parsed)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    contents: Vec<Content>,
    generation_config: GenerationConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    // MAX_TOKENS 到達時など parts が欠落することがあるため default で受ける。
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

#[async_trait]
impl TextGenerator for GeminiVertexClassifier {
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
impl LlmClassifier for GeminiVertexClassifier {
    fn model_id(&self) -> String {
        format!("gemini_vertex:{}", self.model)
    }

    /// generateContent に最小のダミーメッセージを投げ、疎通・認証・権限・クォータを検証する。
    async fn health_check(&self) -> Result<(), AppError> {
        let url = self.endpoint_url();
        let token = vertex_common::access_token(&self.service_account).await?;
        let body = GenerateContentRequest {
            system_instruction: None,
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part {
                    text: Some("ping".to_string()),
                }],
            }],
            generation_config: GenerationConfig {
                max_output_tokens: MAX_OUTPUT_TOKENS,
            },
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
                "Vertex AI (Gemini) health check failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_url_uses_google_generate_content() {
        let sa_json = crate::test_helpers::test_sa_json();
        let c = GeminiVertexClassifier::new(
            sa_json,
            "pigeon-mail-xxxxxx",
            "global",
            "gemini-3.5-flash",
        )
        .unwrap();
        assert_eq!(
            c.endpoint_url(),
            "https://aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/global/publishers/google/models/gemini-3.5-flash:generateContent"
        );
    }

    #[test]
    fn test_build_request_shape() {
        let req = GeminiVertexClassifier::build_request("sys", "usr");
        let json = serde_json::to_value(&req).unwrap();
        // system は systemInstruction、user は contents
        assert_eq!(json["systemInstruction"]["parts"][0]["text"], "sys");
        assert_eq!(json["contents"][0]["role"], "user");
        assert_eq!(json["contents"][0]["parts"][0]["text"], "usr");
        assert_eq!(
            json["generationConfig"]["maxOutputTokens"],
            MAX_OUTPUT_TOKENS
        );
        // Claude 用フィールドが混入していないこと
        assert!(json.get("anthropic_version").is_none());
        assert!(json.get("messages").is_none());
    }

    #[test]
    fn test_extract_text_joins_text_parts() {
        let json = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"{\"action\":"},{"text":"\"unclassified\"}"}]}}]}"#;
        let resp: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            GeminiVertexClassifier::extract_text(&resp).unwrap(),
            "{\"action\":\"unclassified\"}"
        );
    }

    #[test]
    fn test_extract_text_ignores_partless_thought() {
        // thoughtSignature のみで text の無い part は無視される
        let json = r#"{"candidates":[{"content":{"role":"model","parts":[{"thoughtSignature":"abc"},{"text":"ok"}]}}]}"#;
        let resp: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(GeminiVertexClassifier::extract_text(&resp).unwrap(), "ok");
    }

    #[test]
    fn test_extract_text_no_candidates_errs() {
        let json = r#"{"candidates":[]}"#;
        let resp: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert!(GeminiVertexClassifier::extract_text(&resp).is_err());
    }

    #[test]
    fn test_extract_text_empty_parts_errs() {
        // MAX_TOKENS で本文が空（思考のみ消費）のケース
        let json = r#"{"candidates":[{"content":{"role":"model"}}]}"#;
        let resp: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert!(GeminiVertexClassifier::extract_text(&resp).is_err());
    }

    #[test]
    fn test_new_rejects_invalid_sa_json() {
        let result = GeminiVertexClassifier::new("not-json", "proj", "global", "gemini-3.5-flash");
        assert!(matches!(result, Err(AppError::MissingApiKey(_))));
    }
}
