import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProjectForm } from "../components/sidebar/ProjectForm";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));
import { open } from "@tauri-apps/plugin-dialog";

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
    expect(mockOnSubmit).toHaveBeenCalledWith("新しい案件", "案件の説明", "#6b7280", undefined);
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
    expect(onSubmit).toHaveBeenCalledWith("案件名", undefined, "#6b7280", undefined);
  });

  it("calls onCancel when cancel button is clicked", () => {
    const onCancel = vi.fn();
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={onCancel} />);
    fireEvent.click(screen.getByText("キャンセル"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("picks a folder and passes it to onSubmit", async () => {
    vi.mocked(open).mockResolvedValue("/tmp/stage-a");
    const onSubmit = vi.fn();
    render(<ProjectForm onSubmit={onSubmit} onCancel={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /フォルダを選択/ }));
    await screen.findByText("/tmp/stage-a"); // 選択済みパスの表示を待つ

    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "春公演" },
    });
    fireEvent.submit(screen.getByRole("button", { name: "作成" }).closest("form")!);

    expect(onSubmit).toHaveBeenCalledWith("春公演", undefined, "#6b7280", "/tmp/stage-a");
  });

  it("submits without folder when none picked", () => {
    const onSubmit = vi.fn();
    render(<ProjectForm onSubmit={onSubmit} onCancel={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "春公演" },
    });
    fireEvent.submit(screen.getByRole("button", { name: "作成" }).closest("form")!);
    expect(onSubmit).toHaveBeenCalledWith("春公演", undefined, "#6b7280", undefined);
  });
});
