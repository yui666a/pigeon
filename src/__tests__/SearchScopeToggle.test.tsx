import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SearchScopeToggle } from "../components/sidebar/SearchScopeToggle";
import { useSearchStore } from "../stores/searchStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));

describe("SearchScopeToggle", () => {
  beforeEach(() => {
    useSearchStore.setState({
      mode: "fulltext",
      query: "",
      results: [],
      searching: false,
      selectedIndex: -1,
      scopeToProject: false,
    });
  });

  it("案件が選択されていないときは何も描画しない", () => {
    const { container } = render(<SearchScopeToggle selectedProjectId={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("案件選択中はトグルを表示し、デフォルトはOFF", () => {
    render(<SearchScopeToggle selectedProjectId="p1" />);
    const toggle = screen.getByRole("checkbox", { name: "この案件内で検索" });
    expect(toggle).not.toBeChecked();
  });

  it("クリックでON/OFFを切り替える", () => {
    render(<SearchScopeToggle selectedProjectId="p1" />);
    const toggle = screen.getByRole("checkbox", { name: "この案件内で検索" });
    fireEvent.click(toggle);
    expect(useSearchStore.getState().scopeToProject).toBe(true);
    fireEvent.click(toggle);
    expect(useSearchStore.getState().scopeToProject).toBe(false);
  });
});
