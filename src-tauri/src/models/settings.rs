use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmSettings {
    pub provider: String,
    pub ollama_endpoint: String,
    pub ollama_model: String,
    pub claude_model: String,
    /// APIキー本体は返さない。登録済みかどうかのみ。
    pub claude_api_key_set: bool,
    // --- Vertex AI 共通（claude_vertex / gemini_vertex）---
    pub vertex_project_id: String,
    pub vertex_location: String,
    pub vertex_model: String,
    /// SA JSON 本体は返さない。登録済みかどうかのみ。
    pub vertex_sa_json_set: bool,
    // --- Gemini on Vertex AI (gemini_vertex)。SA/project/location は上記と共通 ---
    pub gemini_model: String,
}
