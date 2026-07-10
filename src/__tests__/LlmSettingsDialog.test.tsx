import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { LlmSettingsDialog } from "../components/sidebar/LlmSettingsDialog";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const baseSettings = {
  provider: "ollama",
  ollama_endpoint: "http://localhost:11434",
  ollama_model: "llama3.1:8b",
  claude_model: "claude-haiku-4-5",
  claude_api_key_set: false,
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "get_llm_settings") return Promise.resolve(baseSettings);
    return Promise.resolve();
  });
});

describe("LlmSettingsDialog", () => {
  it("初期表示で現在のプロバイダを読み込む", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    expect(screen.getByLabelText("Ollama（ローカル）")).toBeChecked();
  });

  it("Claudeを選ぶと警告バナーとAPIキー入力が出る", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByLabelText("Claude API"));
    expect(screen.getByText(/クラウドLLMを使用します/)).toBeInTheDocument();
    expect(screen.getByLabelText("Claude APIキー")).toBeInTheDocument();
  });

  it("ChatGPTは選択できない（disabled）", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    expect(screen.getByLabelText(/ChatGPT/)).toBeDisabled();
  });

  it("接続テストボタンで test_llm_connection を呼ぶ", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByRole("button", { name: "接続テスト" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("test_llm_connection"),
    );
  });
});
