import { invokeCommand } from "./client";
import type { LlmProvider, LlmSettings } from "../types/settings";

/**
 * set_llm_settings / test_llm_connection へ渡す引数。
 * 秘密情報（APIキー・SA JSON）は null のときバックエンドが保存済みの値を使う。
 */
export interface LlmSettingsPayload {
  provider: LlmProvider;
  ollamaEndpoint: string;
  ollamaModel: string;
  claudeModel: string;
  claudeApiKey: string | null;
  vertexProjectId: string;
  vertexLocation: string;
  vertexModel: string;
  vertexSaJson: string | null;
  geminiModel: string;
}

/** LLM 設定系 Tauri commands の型付きラッパ */
export const settingsApi = {
  fetchLlmSettings: () => invokeCommand<LlmSettings>("get_llm_settings"),

  setLlmSettings: (payload: LlmSettingsPayload) =>
    invokeCommand<void>("set_llm_settings", { ...payload }),

  testLlmConnection: (payload: LlmSettingsPayload) =>
    invokeCommand<void>("test_llm_connection", { ...payload }),
};
