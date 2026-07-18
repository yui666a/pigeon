//! 埋め込み生成の抽象。本番は Ollama /api/embed（バッチ対応・L2正規化済みを返す）。
//! モデル・次元・プレフィックスは settings で差し替え可能（設計書: モデル変更時は
//! vec_chunks を作り直して全再埋め込み）。

use crate::error::AppError;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

pub mod worker;

/// 埋め込み専用の HTTP クライアントを構築する（classifier::build_http_client と
/// 構造は同じ、タイムアウトのみ異なる）。
/// classifier のチャット用クライアントは 30 秒だが、埋め込みは bge-m3 の
/// コールドロード＋16件バッチの合計で 30 秒を正当に超えうる。同じ 30 秒に
/// 揃えると reqwest タイムアウトが OllamaConnection にマップされ、バックフィルが
/// 無進捗のままキューに滞留し続ける（cold load を毎回払ってゼロ進捗）ため、
/// 埋め込みには専用の長めのタイムアウトを設ける。
fn build_embedding_http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::HttpRequest(e.to_string()))
}

#[async_trait]
pub trait Embedder: Send + Sync {
    fn dimensions(&self) -> usize;
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError>;
}

pub struct OllamaEmbedder {
    endpoint: String,
    model: String,
    dimensions: usize,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

pub(crate) fn build_embed_request(model: &str, inputs: &[String]) -> serde_json::Value {
    json!({ "model": model, "input": inputs })
}

/// レスポンスを検証つきでパースする。件数・次元の不一致は InvalidLlmResponse。
pub(crate) fn parse_embed_response(
    body: &str,
    expected_count: usize,
    expected_dims: usize,
) -> Result<Vec<Vec<f32>>, AppError> {
    let parsed: EmbedResponse = serde_json::from_str(body)
        .map_err(|e| AppError::InvalidLlmResponse(format!("embed response parse: {e}")))?;
    if parsed.embeddings.len() != expected_count {
        return Err(AppError::InvalidLlmResponse(format!(
            "embed count mismatch: got {}, expected {}",
            parsed.embeddings.len(),
            expected_count
        )));
    }
    for e in &parsed.embeddings {
        if e.len() != expected_dims {
            return Err(AppError::InvalidLlmResponse(format!(
                "embed dims mismatch: got {}, expected {}",
                e.len(),
                expected_dims
            )));
        }
    }
    Ok(parsed.embeddings)
}

impl OllamaEmbedder {
    /// build_embedding_http_client は Result を返すため new も Result
    pub fn new(endpoint: String, model: String, dimensions: usize) -> Result<Self, AppError> {
        Ok(Self {
            endpoint,
            model,
            dimensions,
            client: build_embedding_http_client()?,
        })
    }

    /// settings から endpoint/model/dimensions を読んで構築する。
    /// キーと既定値: ollama_endpoint(既存) / embedding_model="bge-m3" /
    /// embedding_dimensions=1024
    pub fn from_settings(conn: &rusqlite::Connection) -> Result<Self, AppError> {
        use crate::db::settings;
        let endpoint = settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434")?;
        let model = settings::get_or_default(conn, "embedding_model", "bge-m3")?;
        let dimensions = settings::get_u32_or(conn, "embedding_dimensions", 1024)? as usize;
        Self::new(endpoint, model, dimensions)
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/api/embed", self.endpoint.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&build_embed_request(&self.model, inputs))
            .send()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(AppError::OllamaConnection(format!(
                "embed HTTP {}",
                resp.status()
            )));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::OllamaConnection(e.to_string()))?;
        parse_embed_response(&body, inputs.len(), self.dimensions)
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;

    /// 決定的なフェイク埋め込み。同じ入力は同じベクトル、字面が近いほど
    /// 近いベクトルにはならない（一致検索のテスト専用）。fail_always を
    /// 立てると常に OllamaConnection エラー（キュー滞留のテスト用）。
    pub struct FakeEmbedder {
        pub dims: usize,
        pub fail_always: bool,
    }

    #[async_trait]
    impl Embedder for FakeEmbedder {
        fn dimensions(&self) -> usize {
            self.dims
        }

        async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            if self.fail_always {
                return Err(AppError::OllamaConnection("fake down".into()));
            }
            Ok(inputs
                .iter()
                .map(|s| {
                    let mut v = vec![0.0f32; self.dims];
                    for (i, b) in s.bytes().enumerate() {
                        v[i % self.dims] += f32::from(b) / 255.0;
                    }
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                    v.iter_mut().for_each(|x| *x /= norm);
                    v
                })
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_embed_request_shape() {
        let req = build_embed_request("bge-m3", &["a".into(), "b".into()]);
        assert_eq!(req["model"], "bge-m3");
        assert_eq!(req["input"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_parse_embed_response_ok() {
        let body = r#"{"embeddings": [[0.1, 0.2], [0.3, 0.4]]}"#;
        let v = parse_embed_response(body, 2, 2).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], vec![0.1, 0.2]);
    }

    #[test]
    fn test_parse_embed_response_count_mismatch_is_error() {
        let body = r#"{"embeddings": [[0.1, 0.2]]}"#;
        assert!(parse_embed_response(body, 2, 2).is_err());
    }

    #[test]
    fn test_parse_embed_response_dims_mismatch_is_error() {
        let body = r#"{"embeddings": [[0.1, 0.2, 0.3]]}"#;
        assert!(parse_embed_response(body, 1, 2).is_err());
    }

    #[test]
    fn test_parse_embed_response_invalid_json_is_error() {
        assert!(parse_embed_response("not json", 1, 2).is_err());
    }
}
