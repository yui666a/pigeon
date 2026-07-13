//! Vertex AI（GCP Agent Platform）共通処理。
//!
//! Claude on Vertex（`claude_vertex.rs`）と Gemini on Vertex（`gemini_vertex.rs`）で
//! 共通する SA JSON パース・アクセストークン取得・エンドポイント URL 組み立てを集約する。
//! publisher（`anthropic` / `google`）と RPC メソッド（`rawPredict` / `generateContent`）
//! だけが呼び出し側で異なる。

use gcp_auth::{CustomServiceAccount, TokenProvider};

use crate::error::AppError;

const GCP_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// SA JSON をパースする。`provider` はエラーメッセージ用のラベル（例: `claude_vertex`）。
pub(crate) fn parse_service_account(
    sa_json: &str,
    provider: &str,
) -> Result<CustomServiceAccount, AppError> {
    CustomServiceAccount::from_json(sa_json)
        .map_err(|e| AppError::MissingApiKey(format!("{provider} (invalid SA JSON: {e})")))
}

/// SA からアクセストークンを取得する（クレート側でキャッシュ・失効管理される）。
pub(crate) async fn access_token(
    service_account: &CustomServiceAccount,
) -> Result<String, AppError> {
    let token = service_account
        .token(&[GCP_SCOPE])
        .await
        .map_err(|e| AppError::HttpRequest(format!("Vertex token error: {e}")))?;
    Ok(token.as_str().to_string())
}

/// Vertex AI のモデル呼び出しエンドポイント URL を組み立てる。
///
/// `global` はホスト名が特殊で、`global-aiplatform...` ではなく
/// `aiplatform.googleapis.com`（プレフィックス無し）になる。それ以外の
/// リージョンは `{location}-aiplatform.googleapis.com`。
pub(crate) fn endpoint_url(
    location: &str,
    project_id: &str,
    publisher: &str,
    model: &str,
    method: &str,
) -> String {
    let host = if location == "global" {
        "aiplatform.googleapis.com".to_string()
    } else {
        format!("{location}-aiplatform.googleapis.com")
    };
    format!(
        "https://{host}/v1/projects/{project_id}/locations/{location}/publishers/{publisher}/models/{model}:{method}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_url_regional_has_region_prefixed_host() {
        let url = endpoint_url(
            "us-east5",
            "pigeon-mail-xxxxxx",
            "anthropic",
            "claude-haiku-4-5@20251001",
            "rawPredict",
        );
        assert_eq!(
            url,
            "https://us-east5-aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/us-east5/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict"
        );
    }

    #[test]
    fn test_endpoint_url_global_has_no_region_prefix() {
        // global はホスト名にリージョン接頭辞が付かない。
        let url = endpoint_url(
            "global",
            "pigeon-mail-xxxxxx",
            "anthropic",
            "claude-haiku-4-5@20251001",
            "rawPredict",
        );
        assert_eq!(
            url,
            "https://aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/global/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict"
        );
    }

    #[test]
    fn test_endpoint_url_google_publisher_generate_content() {
        let url = endpoint_url(
            "us-central1",
            "proj",
            "google",
            "gemini-3.5-flash",
            "generateContent",
        );
        assert_eq!(
            url,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/proj/locations/us-central1/publishers/google/models/gemini-3.5-flash:generateContent"
        );
    }

    #[test]
    fn test_endpoint_url_google_publisher_global() {
        let url = endpoint_url(
            "global",
            "pigeon-mail-xxxxxx",
            "google",
            "gemini-3.5-flash",
            "generateContent",
        );
        assert_eq!(
            url,
            "https://aiplatform.googleapis.com/v1/projects/pigeon-mail-xxxxxx/locations/global/publishers/google/models/gemini-3.5-flash:generateContent"
        );
    }

    #[test]
    fn test_parse_service_account_rejects_invalid_json_with_provider_label() {
        let result = parse_service_account("not-json", "claude_vertex");
        match result {
            Err(AppError::MissingApiKey(msg)) => {
                assert!(msg.starts_with("claude_vertex"));
                assert!(msg.contains("invalid SA JSON"));
            }
            other => panic!("expected MissingApiKey error, got {other:?}"),
        }
    }
}
