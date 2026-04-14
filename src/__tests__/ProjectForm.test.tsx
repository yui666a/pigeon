import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProjectForm } from "../components/sidebar/ProjectForm";

describe("ProjectForm", () => {
  const mockOnSubmit = vi.fn();
  const mockOnCancel = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders all form fields", () => {
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />);
    expect(screen.getByPlaceholderText("案件名を入力")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("説明（任意）")).toBeInTheDocument();
    expect(screen.getByText("作成")).toBeInTheDocument();
    expect(screen.getByText("キャンセル")).toBeInTheDocument();
  });

  it("calls onSubmit with trimmed name and description", () => {
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />);
    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), { target: { value: "  新しい案件  " } });
    fireEvent.change(screen.getByPlaceholderText("説明（任意）"), { target: { value: "案件の説明" } });
    fireEvent.click(screen.getByText("作成"));
    expect(mockOnSubmit).toHaveBeenCalledWith("新しい案件", "案件の説明", "#6b7280");
  });

  it("does not submit with empty name", () => {
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />);
    fireEvent.click(screen.getByText("作成"));
    expect(mockOnSubmit).not.toHaveBeenCalled();
  });

  it("passes undefined description when empty", () => {
    const onSubmit = vi.fn();
    render(<ProjectForm onSubmit={onSubmit} onCancel={mockOnCancel} />);
    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), { target: { value: "案件名" } });
    fireEvent.click(screen.getByText("作成"));
    expect(onSubmit).toHaveBeenCalledWith("案件名", undefined, "#6b7280");
  });

  it("calls onCancel when cancel button is clicked", () => {
    const onCancel = vi.fn();
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={onCancel} />);
    fireEvent.click(screen.getByText("キャンセル"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
