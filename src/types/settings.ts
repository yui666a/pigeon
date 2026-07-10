export type LlmProvider = "ollama" | "claude" | "openai";

export interface LlmSettings {
  provider: LlmProvider;
  ollama_endpoint: string;
  ollama_model: string;
  claude_model: string;
  claude_api_key_set: boolean;
}
