import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LlmProvider, LlmSettings } from "../../types/settings";
import { useErrorStore } from "../../stores/errorStore";

interface Props {
  onClose: () => void;
}

const PROVIDERS: { value: LlmProvider; label: string; disabled?: boolean }[] = [
  { value: "ollama", label: "Ollama（ローカル）" },
  { value: "claude", label: "Claude API" },
  { value: "openai", label: "ChatGPT（未対応・今後対応予定）", disabled: true },
];

export function LlmSettingsDialog({ onClose }: Props) {
  const [settings, setSettings] = useState<LlmSettings | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
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

  const handleSave = async () => {
    if (!settings) return;
    try {
      await invoke("set_llm_settings", {
        provider: settings.provider,
        ollamaEndpoint: settings.ollama_endpoint,
        ollamaModel: settings.ollama_model,
        claudeModel: settings.claude_model,
        claudeApiKey: apiKeyInput === "" ? null : apiKeyInput,
      });
      onClose();
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const handleTest = async () => {
    setTestResult(null);
    try {
      await invoke("test_llm_connection");
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
