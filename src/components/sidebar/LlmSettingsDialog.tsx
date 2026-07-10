import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LlmProvider, LlmSettings } from "../../types/settings";
import { useErrorStore } from "../../stores/errorStore";

interface Props {
  onClose: () => void;
}

const PROVIDERS: { value: LlmProvider; label: string; disabled?: boolean }[] = [
  { value: "ollama", label: "Ollama（ローカル）" },
  { value: "claude", label: "Claude API（Anthropic 直）" },
  { value: "claude_vertex", label: "Claude (GCP Vertex AI)" },
  { value: "gemini_vertex", label: "Gemini (GCP Vertex AI)" },
  { value: "openai", label: "ChatGPT（未対応・今後対応予定）", disabled: true },
];

export function LlmSettingsDialog({ onClose }: Props) {
  const [settings, setSettings] = useState<LlmSettings | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [saJsonInput, setSaJsonInput] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const s = await invoke<LlmSettings>("get_llm_settings");
        setSettings(s);
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
    })();
  }, []);

  const update = useCallback(
    <K extends keyof LlmSettings>(key: K, value: LlmSettings[K]) => {
      setSettings((prev) => (prev ? { ...prev, [key]: value } : prev));
    },
    [],
  );

  // set/test で共通のペイロード。秘密情報（APIキー・SA JSON）は新規入力があればそれを、
  // 空ならバックエンドが保存済みの値を使う（＝null を渡す）。
  const buildPayload = (s: LlmSettings) => ({
    provider: s.provider,
    ollamaEndpoint: s.ollama_endpoint,
    ollamaModel: s.ollama_model,
    claudeModel: s.claude_model,
    claudeApiKey: apiKeyInput === "" ? null : apiKeyInput,
    vertexProjectId: s.vertex_project_id,
    vertexLocation: s.vertex_location,
    vertexModel: s.vertex_model,
    vertexSaJson: saJsonInput === "" ? null : saJsonInput,
    geminiModel: s.gemini_model,
  });

  const handleSave = async () => {
    if (!settings) return;
    try {
      await invoke("set_llm_settings", buildPayload(settings));
      onClose();
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const handleTest = async () => {
    if (!settings) return;
    setTestResult(null);
    try {
      // 保存済み設定ではなく、いま画面で選んでいる設定でテストする。
      await invoke("test_llm_connection", buildPayload(settings));
      setTestResult("接続成功");
    } catch (e) {
      setTestResult(`接続失敗: ${String(e)}`);
    }
  };

  if (!settings) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
        <div className="rounded-lg bg-white px-6 py-4 text-sm text-gray-500">
          読み込み中…
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex max-h-[80vh] w-[520px] flex-col rounded-lg bg-white shadow-xl">
        <div className="border-b px-5 py-3">
          <h2 className="text-sm font-bold">LLM設定</h2>
        </div>
        <div className="flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <fieldset className="space-y-2">
            <legend className="text-xs font-semibold uppercase tracking-wide text-gray-400">
              プロバイダ
            </legend>
            {PROVIDERS.map((p) => (
              <label key={p.value} className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name="llm-provider"
                  aria-label={p.label}
                  checked={settings.provider === p.value}
                  disabled={p.disabled}
                  onChange={() => update("provider", p.value)}
                />
                <span className={p.disabled ? "text-gray-400" : ""}>{p.label}</span>
              </label>
            ))}
          </fieldset>

          {settings.provider === "ollama" && (
            <div className="space-y-2">
              <label className="block text-sm">
                エンドポイント
                <input
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  value={settings.ollama_endpoint}
                  onChange={(e) => update("ollama_endpoint", e.target.value)}
                />
              </label>
              <label className="block text-sm">
                モデル
                <input
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  value={settings.ollama_model}
                  onChange={(e) => update("ollama_model", e.target.value)}
                />
              </label>
            </div>
          )}

          {settings.provider === "claude" && (
            <div className="space-y-2">
              <p className="rounded bg-amber-50 px-3 py-2 text-xs text-amber-700">
                クラウドLLMを使用します。件名・送信者・本文冒頭300文字と、許可した案件コンテキストが
                Anthropic に送信されます。
              </p>
              <label className="block text-sm">
                Claude APIキー
                <input
                  type="password"
                  aria-label="Claude APIキー"
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  placeholder={
                    settings.claude_api_key_set ? "••••••••（登録済み・変更時のみ入力）" : "sk-ant-..."
                  }
                  value={apiKeyInput}
                  onChange={(e) => setApiKeyInput(e.target.value)}
                />
              </label>
              <label className="block text-sm">
                モデル
                <input
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  placeholder="claude-haiku-4-5"
                  value={settings.claude_model}
                  onChange={(e) => update("claude_model", e.target.value)}
                />
              </label>
            </div>
          )}

          {(settings.provider === "claude_vertex" ||
            settings.provider === "gemini_vertex") && (
            <div className="space-y-2">
              <p className="rounded bg-amber-50 px-3 py-2 text-xs text-amber-700">
                クラウドLLMを使用します。件名・送信者・本文冒頭300文字と、許可した案件コンテキストが
                Google Cloud (Vertex AI) 上の
                {settings.provider === "gemini_vertex" ? " Gemini" : " Claude"}{" "}
                に送信されます。
              </p>
              <label className="block text-sm">
                サービスアカウント JSON キー
                <textarea
                  aria-label="サービスアカウント JSON キー"
                  className="mt-1 h-24 w-full rounded border px-2 py-1 font-mono text-xs"
                  placeholder={
                    settings.vertex_sa_json_set
                      ? "登録済み・変更時のみ貼り付け"
                      : '{ "type": "service_account", ... }'
                  }
                  value={saJsonInput}
                  onChange={(e) => setSaJsonInput(e.target.value)}
                />
              </label>
              <label className="block text-sm">
                プロジェクト ID
                <input
                  aria-label="プロジェクト ID"
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  placeholder="my-gcp-project"
                  value={settings.vertex_project_id}
                  onChange={(e) => update("vertex_project_id", e.target.value)}
                />
              </label>
              <label className="block text-sm">
                リージョン
                <input
                  aria-label="リージョン"
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  placeholder="global"
                  value={settings.vertex_location}
                  onChange={(e) => update("vertex_location", e.target.value)}
                />
              </label>
              {settings.provider === "claude_vertex" ? (
                <label className="block text-sm">
                  モデル
                  <input
                    className="mt-1 w-full rounded border px-2 py-1 text-sm"
                    placeholder="claude-haiku-4-5@20251001"
                    value={settings.vertex_model}
                    onChange={(e) => update("vertex_model", e.target.value)}
                  />
                </label>
              ) : (
                <label className="block text-sm">
                  モデル
                  <input
                    className="mt-1 w-full rounded border px-2 py-1 text-sm"
                    placeholder="gemini-3.5-flash"
                    value={settings.gemini_model}
                    onChange={(e) => update("gemini_model", e.target.value)}
                  />
                </label>
              )}
            </div>
          )}

          <div className="flex items-center gap-3">
            <button
              onClick={() => void handleTest()}
              className="rounded border border-gray-300 px-3 py-1.5 text-sm hover:bg-gray-50"
            >
              接続テスト
            </button>
            {testResult && (
              <span className="text-xs text-gray-600">{testResult}</span>
            )}
          </div>
        </div>
        <div className="flex justify-end gap-2 border-t px-5 py-3">
          <button
            onClick={onClose}
            className="rounded px-4 py-1.5 text-sm text-gray-600 hover:bg-gray-100"
          >
            キャンセル
          </button>
          <button
            onClick={() => void handleSave()}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
