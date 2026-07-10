use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmSettings {
    pub provider: String,
    pub ollama_endpoint: String,
    pub ollama_model: String,
    pub claude_model: String,
    /// APIキー本体は返さない。登録済みかどうかのみ。
    pub claude_api_key_set: bool,
}
