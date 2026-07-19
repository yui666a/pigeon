import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ProjectNotePanel } from "../components/project-note/ProjectNotePanel";
import { useProjectNoteStore } from "../stores/projectNoteStore";

vi.mock("../stores/projectNoteStore");

const baseNote = {
  project_id: "p1",
  user_md: "会場メモ",
  ai_md: "- 公演: 春公演",
  ai_edited: false,
  ai_generated_at: "2026-07-19T10:00:00Z",
  updated_at: null,
};

function mockStore(overrides: Record<string, unknown> = {}) {
  const state = {
    note: baseNote,
    history: [],
    loading: false,
    generating: false,
    load: vi.fn(),
    saveUser: vi.fn(),
    saveAi: vi.fn(),
    generate: vi.fn(),
    loadHistory: vi.fn(),
    restore: vi.fn(),
    ...overrides,
  };
  vi.mocked(useProjectNoteStore).mockReturnValue(state);
  return state;
}

describe("ProjectNotePanel", () => {
  beforeEach(() => vi.clearAllMocks());

  it("ノートタブとAI要約タブを表示する", () => {
    mockStore();
    render(<ProjectNotePanel projectId="p1" />);
    expect(screen.getByRole("tab", { name: "ノート" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "AI要約" })).toBeInTheDocument();
  });

  it("初期表示はノートタブ", () => {
    mockStore();
    render(<ProjectNotePanel projectId="p1" />);
    expect(screen.getByLabelText("案件ノート")).toBeInTheDocument();
  });

  it("AI要約タブへ切り替えられる", () => {
    mockStore();
    render(<ProjectNotePanel projectId="p1" />);
    fireEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    expect(screen.getByLabelText("AI要約")).toBeInTheDocument();
  });

  it("手修正が無ければ確認なしで生成する", async () => {
    const s = mockStore({ note: { ...baseNote, ai_edited: false } });
    render(<ProjectNotePanel projectId="p1" />);
    fireEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    fireEvent.click(screen.getByRole("button", { name: /再生成/ }));
    await waitFor(() => expect(s.generate).toHaveBeenCalledWith("p1"));
  });

  it("手修正があれば確認ダイアログを出し、承認するまで生成しない", async () => {
    const s = mockStore({ note: { ...baseNote, ai_edited: true } });
    render(<ProjectNotePanel projectId="p1" />);
    fireEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    fireEvent.click(screen.getByRole("button", { name: /再生成/ }));

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(s.generate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "上書きする" }));
    await waitFor(() => expect(s.generate).toHaveBeenCalledWith("p1"));
  });

  it("確認ダイアログをキャンセルすると生成しない", () => {
    const s = mockStore({ note: { ...baseNote, ai_edited: true } });
    render(<ProjectNotePanel projectId="p1" />);
    fireEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    fireEvent.click(screen.getByRole("button", { name: /再生成/ }));
    fireEvent.click(screen.getByRole("button", { name: "キャンセル" }));

    expect(s.generate).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("生成中はボタンを無効化する", () => {
    mockStore({ generating: true });
    render(<ProjectNotePanel projectId="p1" />);
    fireEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    expect(screen.getByRole("button", { name: /生成中/ })).toBeDisabled();
  });

  it("履歴から復元できる", async () => {
    const s = mockStore({
      history: [
        {
          id: "h1",
          project_id: "p1",
          ai_md: "以前の要約",
          replaced_at: "2026-07-18T10:00:00Z",
        },
      ],
    });
    render(<ProjectNotePanel projectId="p1" />);
    fireEvent.click(screen.getByRole("tab", { name: "AI要約" }));
    fireEvent.click(screen.getByRole("button", { name: /履歴/ }));
    fireEvent.click(screen.getByRole("button", { name: "この版に戻す" }));
    await waitFor(() => expect(s.restore).toHaveBeenCalledWith("p1", "h1"));
  });
});
