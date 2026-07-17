import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { NewProjectFromSelectionForm } from "../components/thread-list/NewProjectFromSelectionForm";
import { classifyApi } from "../api/classifyApi";

vi.mock("../api/classifyApi", () => ({
  classifyApi: { suggestProjectFromMails: vi.fn() },
}));

const suggestMock = vi.mocked(classifyApi.suggestProjectFromMails);

describe("NewProjectFromSelectionForm", () => {
  beforeEach(() => vi.clearAllMocks());

  it("提案取得中はローディングを表示する", () => {
    suggestMock.mockReturnValue(new Promise(() => {})); // 未解決
    render(
      <NewProjectFromSelectionForm mailIds={["m1"]} onCreate={() => {}} onCancel={() => {}} />,
    );
    expect(screen.getByText(/提案を取得中|案件名を提案中/)).toBeInTheDocument();
  });

  it("提案結果を名前・説明の初期値に反映する", async () => {
    suggestMock.mockResolvedValue({ name: "在庫管理", description: "在庫の件" });
    render(
      <NewProjectFromSelectionForm mailIds={["m1", "m2"]} onCreate={() => {}} onCancel={() => {}} />,
    );
    await waitFor(() =>
      expect(screen.getByDisplayValue("在庫管理")).toBeInTheDocument(),
    );
    expect(screen.getByDisplayValue("在庫の件")).toBeInTheDocument();
  });

  it("名前が空だと作成ボタンが無効", async () => {
    suggestMock.mockResolvedValue({ name: "", description: "" });
    render(
      <NewProjectFromSelectionForm mailIds={["m1"]} onCreate={() => {}} onCancel={() => {}} />,
    );
    await waitFor(() => expect(suggestMock).toHaveBeenCalled());
    const createBtn = screen.getByRole("button", { name: /作成/ });
    expect(createBtn).toBeDisabled();
  });

  it("作成クリックで onCreate に入力値を渡す", async () => {
    suggestMock.mockResolvedValue({ name: "在庫管理", description: "在庫の件" });
    const onCreate = vi.fn();
    render(
      <NewProjectFromSelectionForm mailIds={["m1"]} onCreate={onCreate} onCancel={() => {}} />,
    );
    await waitFor(() => expect(screen.getByDisplayValue("在庫管理")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /作成/ }));
    expect(onCreate).toHaveBeenCalledWith("在庫管理", "在庫の件");
  });

  it("提案が失敗しても空フォームを表示し手入力で作成できる", async () => {
    suggestMock.mockRejectedValue(new Error("llm down"));
    const onCreate = vi.fn();
    render(
      <NewProjectFromSelectionForm mailIds={["m1"]} onCreate={onCreate} onCancel={() => {}} />,
    );
    await waitFor(() =>
      expect(screen.getByPlaceholderText("案件名を入力")).toBeInTheDocument(),
    );
    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "手入力案件" },
    });
    fireEvent.click(screen.getByRole("button", { name: /作成/ }));
    expect(onCreate).toHaveBeenCalledWith("手入力案件", undefined);
  });
});
