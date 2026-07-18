import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SearchModeToggle } from "../components/sidebar/SearchModeToggle";
import { useSearchStore } from "../stores/searchStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));

describe("SearchModeToggle", () => {
  beforeEach(() => {
    localStorage.clear();
    useSearchStore.setState({ mode: "fulltext", query: "", results: [], searching: false, selectedIndex: -1 });
  });

  it("両モードのボタンを表示し現在モードを強調する", () => {
    render(<SearchModeToggle />);
    expect(screen.getByRole("button", { name: "文字列" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "ベクトル" })).toHaveAttribute("aria-pressed", "false");
  });

  it("クリックでモードを切り替える", () => {
    render(<SearchModeToggle />);
    fireEvent.click(screen.getByRole("button", { name: "ベクトル" }));
    expect(useSearchStore.getState().mode).toBe("semantic");
  });

  it("検索中のクエリがあればモード切替で再検索する", () => {
    const searchSpy = vi.fn();
    useSearchStore.setState({ query: "照明", search: searchSpy });
    render(<SearchModeToggle accountId="acc1" />);
    fireEvent.click(screen.getByRole("button", { name: "ベクトル" }));
    expect(searchSpy).toHaveBeenCalledWith("acc1", "照明");
  });
});
