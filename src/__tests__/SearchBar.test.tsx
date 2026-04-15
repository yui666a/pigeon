import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SearchBar } from "../components/sidebar/SearchBar";

describe("SearchBar", () => {
  const mockOnSearch = vi.fn();
  const mockOnClear = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders search input", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    expect(screen.getByPlaceholderText("検索...")).toBeInTheDocument();
  });

  it("calls onSearch when Enter is pressed", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    const input = screen.getByPlaceholderText("検索...");
    fireEvent.change(input, { target: { value: "test query" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(mockOnSearch).toHaveBeenCalledWith("test query");
  });

  it("calls onClear when Escape is pressed", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    const input = screen.getByPlaceholderText("検索...");
    fireEvent.change(input, { target: { value: "something" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(mockOnClear).toHaveBeenCalled();
  });

  it("does not call onSearch for empty query", () => {
    render(<SearchBar onSearch={mockOnSearch} onClear={mockOnClear} />);
    const input = screen.getByPlaceholderText("検索...");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(mockOnSearch).not.toHaveBeenCalled();
  });
});
