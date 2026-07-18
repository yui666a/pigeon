export type LlmProvider =
  | "ollama"
  | "claude"
  | "claude_vertex"
  | "gemini_vertex"
  | "openai";

export interface LlmSettings {
  provider: LlmProvider;
  ollama_endpoint: string;
  ollama_model: string;
  claude_model: string;
  claude_api_key_set: boolean;
  // Vertex AI 共通 (claude_vertex / gemini_vertex)
  vertex_project_id: string;
  vertex_location: string;
  vertex_model: string;
  vertex_sa_json_set: boolean;
  // Gemini on Vertex AI (gemini_vertex)。SA/project/location は上記と共通
  gemini_model: string;
  // 埋め込みモデル（セマンティック検索用）。次元・プレフィックスは v1 では非公開
  embedding_model: string;
}
