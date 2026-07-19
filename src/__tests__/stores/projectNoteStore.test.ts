import { describe, it, expect, beforeEach, vi } from "vitest";
import { useProjectNoteStore } from "../../stores/projectNoteStore";
import { useErrorStore } from "../../stores/errorStore";

vi.mock("../../api/projectNoteApi", () => ({
  projectNoteApi: {
    fetchNote: vi.fn(),
    saveUserNote: vi.fn(),
    saveAiNote: vi.fn(),
    generateAiNote: vi.fn(),
    fetchAiHistory: vi.fn(),
    restoreAiNote: vi.fn(),
  },
}));

import { projectNoteApi } from "../../api/projectNoteApi";

const emptyNote = {
  project_id: "p1",
  user_md: "",
  ai_md: null,
  ai_edited: false,
  ai_generated_at: null,
  updated_at: null,
};

describe("projectNoteStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useProjectNoteStore.setState({
      note: null,
      history: [],
      loading: false,
      generating: false,
    });
    useErrorStore.setState({ toasts: [] });
  });

  it("load はノートをストアへ入れる", async () => {
    vi.mocked(projectNoteApi.fetchNote).mockResolvedValue({
      ...emptyNote,
      user_md: "会場メモ",
    });
    await useProjectNoteStore.getState().load("p1");
    expect(useProjectNoteStore.getState().note?.user_md).toBe("会場メモ");
    expect(useProjectNoteStore.getState().loading).toBe(false);
  });

  it("ノート未作成なら空ノートとして扱う", async () => {
    vi.mocked(projectNoteApi.fetchNote).mockResolvedValue(null);
    await useProjectNoteStore.getState().load("p1");
    const note = useProjectNoteStore.getState().note;
    expect(note).not.toBeNull();
    expect(note?.user_md).toBe("");
    expect(note?.project_id).toBe("p1");
  });

  it("saveUser は保存後にローカル状態を更新する", async () => {
    vi.mocked(projectNoteApi.fetchNote).mockResolvedValue(emptyNote);
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(projectNoteApi.saveUserNote).mockResolvedValue(undefined);

    await useProjectNoteStore.getState().saveUser("p1", "新しいノート");

    expect(projectNoteApi.saveUserNote).toHaveBeenCalledWith("p1", "新しいノート");
    expect(useProjectNoteStore.getState().note?.user_md).toBe("新しいノート");
  });

  it("saveAi は ai_edited を true にする", async () => {
    vi.mocked(projectNoteApi.fetchNote).mockResolvedValue(emptyNote);
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(projectNoteApi.saveAiNote).mockResolvedValue(undefined);

    await useProjectNoteStore.getState().saveAi("p1", "手で直した");

    expect(useProjectNoteStore.getState().note?.ai_edited).toBe(true);
    expect(useProjectNoteStore.getState().note?.ai_md).toBe("手で直した");
  });

  it("generate は結果を反映し ai_edited をリセットする", async () => {
    vi.mocked(projectNoteApi.fetchNote).mockResolvedValue({
      ...emptyNote,
      ai_md: "旧",
      ai_edited: true,
    });
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(projectNoteApi.generateAiNote).mockResolvedValue({
      ai_md: "新しい要約",
      dropped_mails: 0,
    });

    await useProjectNoteStore.getState().generate("p1");

    expect(projectNoteApi.generateAiNote).toHaveBeenCalledWith("p1");
    const s = useProjectNoteStore.getState();
    expect(s.note?.ai_md).toBe("新しい要約");
    expect(s.note?.ai_edited).toBe(false);
    expect(s.generating).toBe(false);
  });

  it("generate 失敗時はトースト通知を出し既存 ai_md を保持する", async () => {
    vi.mocked(projectNoteApi.fetchNote).mockResolvedValue({
      ...emptyNote,
      ai_md: "既存の要約",
    });
    await useProjectNoteStore.getState().load("p1");
    vi.mocked(projectNoteApi.generateAiNote).mockRejectedValue(new Error("LLM失敗"));

    await useProjectNoteStore.getState().generate("p1");

    const s = useProjectNoteStore.getState();
    expect(s.note?.ai_md).toBe("既存の要約");
    expect(s.generating).toBe(false);
    const toasts = useErrorStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ kind: "error", message: "LLM失敗" });
  });

  it("loadHistory は履歴一覧をストアへ入れる", async () => {
    vi.mocked(projectNoteApi.fetchAiHistory).mockResolvedValue([
      {
        id: "h1",
        project_id: "p1",
        ai_md: "過去の要約",
        replaced_at: "2026-07-19T00:00:00Z",
      },
    ]);

    await useProjectNoteStore.getState().loadHistory("p1");

    const s = useProjectNoteStore.getState();
    expect(s.history).toHaveLength(1);
    expect(s.history[0].ai_md).toBe("過去の要約");
  });

  it("restore は復元後にノートと履歴を再読み込みする", async () => {
    vi.mocked(projectNoteApi.fetchNote)
      .mockResolvedValueOnce(emptyNote)
      .mockResolvedValueOnce({ ...emptyNote, ai_md: "復元された要約" });
    await useProjectNoteStore.getState().load("p1");

    vi.mocked(projectNoteApi.restoreAiNote).mockResolvedValue(undefined);
    vi.mocked(projectNoteApi.fetchAiHistory).mockResolvedValue([]);

    await useProjectNoteStore.getState().restore("p1", "h1");

    expect(projectNoteApi.restoreAiNote).toHaveBeenCalledWith("h1");
    expect(projectNoteApi.fetchNote).toHaveBeenCalledTimes(2);
    expect(useProjectNoteStore.getState().note?.ai_md).toBe("復元された要約");
  });
});
