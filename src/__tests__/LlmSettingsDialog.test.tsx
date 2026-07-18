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
  vertex_project_id: "",
  vertex_location: "global",
  vertex_model: "claude-haiku-4-5@20251001",
  vertex_sa_json_set: false,
  gemini_model: "gemini-3.5-flash",
  embedding_model: "bge-m3",
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
    fireEvent.click(screen.getByLabelText("Claude API（Anthropic 直）"));
    expect(screen.getByText(/クラウドLLMを使用します/)).toBeInTheDocument();
    expect(screen.getByLabelText("Claude APIキー")).toBeInTheDocument();
  });

  it("Claude (Vertex) を選ぶとSA JSON・プロジェクトID・リージョン入力と警告が出る", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByLabelText("Claude (GCP Vertex AI)"));
    expect(screen.getByText(/Google Cloud \(Vertex AI\)/)).toBeInTheDocument();
    expect(screen.getByLabelText("サービスアカウント JSON キー")).toBeInTheDocument();
    expect(screen.getByLabelText("プロジェクト ID")).toBeInTheDocument();
    expect(screen.getByLabelText("リージョン")).toBeInTheDocument();
  });

  it("Vertex に切り替えて接続テストすると provider:claude_vertex とvertexフィールドを渡す", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByLabelText("Claude (GCP Vertex AI)"));
    fireEvent.change(screen.getByLabelText("プロジェクト ID"), {
      target: { value: "my-proj" },
    });
    fireEvent.click(screen.getByRole("button", { name: "接続テスト" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "test_llm_connection",
        expect.objectContaining({
          provider: "claude_vertex",
          vertexProjectId: "my-proj",
          vertexLocation: "global",
          vertexModel: "claude-haiku-4-5@20251001",
        }),
      ),
    );
  });

  it("Gemini (Vertex) を選ぶとSA JSON・プロジェクトID入力とモデルgemini欄が出る", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByLabelText("Gemini (GCP Vertex AI)"));
    expect(screen.getByText(/Gemini に送信されます/)).toBeInTheDocument();
    expect(screen.getByLabelText("サービスアカウント JSON キー")).toBeInTheDocument();
    expect(screen.getByLabelText("プロジェクト ID")).toBeInTheDocument();
  });

  it("Gemini に切り替えて接続テストすると provider:gemini_vertex と geminiModel を渡す", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByLabelText("Gemini (GCP Vertex AI)"));
    fireEvent.change(screen.getByLabelText("プロジェクト ID"), {
      target: { value: "my-proj" },
    });
    fireEvent.click(screen.getByRole("button", { name: "接続テスト" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "test_llm_connection",
        expect.objectContaining({
          provider: "gemini_vertex",
          vertexProjectId: "my-proj",
          geminiModel: "gemini-3.5-flash",
        }),
      ),
    );
  });

  it("ChatGPTは選択できない（disabled）", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    expect(screen.getByLabelText(/ChatGPT/)).toBeDisabled();
  });

  it("接続テストは現在の画面設定を渡して test_llm_connection を呼ぶ", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByRole("button", { name: "接続テスト" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "test_llm_connection",
        expect.objectContaining({ provider: "ollama" }),
      ),
    );
  });

  it("Claudeに切り替えて未保存のまま接続テストすると provider:claude で検証する（保存済みollamaにならない）", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    // 保存ボタンを押さずにプロバイダだけ Claude に変更
    fireEvent.click(screen.getByLabelText("Claude API（Anthropic 直）"));
    fireEvent.click(screen.getByRole("button", { name: "接続テスト" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "test_llm_connection",
        expect.objectContaining({ provider: "claude" }),
      ),
    );
    // set_llm_settings（保存）は呼ばれていない＝未保存のままテストしている
    expect(invokeMock).not.toHaveBeenCalledWith(
      "set_llm_settings",
      expect.anything(),
    );
  });
});
