import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SmartViewList } from "../components/sidebar/SmartViewList";
import { useSavedSearchStore } from "../stores/savedSearchStore";
import { useSearchStore } from "../stores/searchStore";
import type { SavedSearch } from "../types/savedSearch";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const saved: SavedSearch = {
  id: 1,
  name: "照明",
  query: "灯体",
  mode: "semantic",
  sort_order: 0,
  created_at: "2026-07-18",
};

describe("SmartViewList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue([]);
    useSavedSearchStore.setState({ savedSearches: [saved], loading: false });
    useSearchStore.setState({
      mode: "fulltext",
      query: "",
      results: [],
      searching: false,
      selectedIndex: -1,
    });
  });

  it("保存済み検索を一覧表示する", () => {
    render(<SmartViewList accountId="acc1" />);
    expect(screen.getByText("スマートビュー")).toBeInTheDocument();
    expect(screen.getByText("照明")).toBeInTheDocument();
  });

  it("0件のときはセクションを表示しない", () => {
    useSavedSearchStore.setState({ savedSearches: [], loading: false });
    const { container } = render(<SmartViewList accountId="acc1" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("クリックで保存されたモード・クエリで検索を実行する", async () => {
    render(<SmartViewList accountId="acc1" />);
    fireEvent.click(screen.getByText("照明"));
    await waitFor(() => {
      expect(useSearchStore.getState().mode).toBe("semantic");
      expect(mockInvoke).toHaveBeenCalledWith("semantic_search", {
        accountId: "acc1",
        query: "灯体",
      });
    });
  });

  it("右クリックメニューから削除できる", async () => {
    render(<SmartViewList accountId="acc1" />);
    fireEvent.contextMenu(screen.getByText("照明"));
    fireEvent.click(screen.getByText("削除"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("delete_saved_search", { id: 1 });
    });
  });

  it("右クリックメニューの名前変更でインライン入力→Enterで確定する", async () => {
    render(<SmartViewList accountId="acc1" />);
    fireEvent.contextMenu(screen.getByText("照明"));
    fireEvent.click(screen.getByText("名前変更"));
    const input = screen.getByDisplayValue("照明");
    fireEvent.change(input, { target: { value: "ライト" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("rename_saved_search", {
        id: 1,
        name: "ライト",
      });
    });
  });

  it("名前変更入力はフォーカス喪失で閉じる", () => {
    render(<SmartViewList accountId="acc1" />);
    fireEvent.contextMenu(screen.getByText("照明"));
    fireEvent.click(screen.getByText("名前変更"));
    const input = screen.getByDisplayValue("照明");
    fireEvent.blur(input);
    expect(screen.queryByDisplayValue("照明")).not.toBeInTheDocument();
    expect(screen.getByText("照明")).toBeInTheDocument();
  });
});
