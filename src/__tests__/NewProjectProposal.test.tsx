import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { NewProjectProposal } from "../components/common/NewProjectProposal";
import type { Project } from "../types/project";

const p = (id: string, name: string, parent: string | null): Project => ({
  id, account_id: "acc1", name, description: null, color: null,
  is_archived: false, parent_id: parent,
  created_at: "2026-07-18", updated_at: "2026-07-18",
});

describe("NewProjectProposal", () => {
  const defaultProps = {
    mailId: "mail-1",
    suggestedName: "新規案件",
    suggestedDescription: "AIが提案した説明",
    reason: "既存プロジェクトに一致なし",
    projects: [] as Project[],
    onApprove: vi.fn(),
    onReject: vi.fn(),
  };

  it("renders reason text and pre-filled form", () => {
    render(<NewProjectProposal {...defaultProps} />);
    expect(screen.getByText("既存プロジェクトに一致なし")).toBeInTheDocument();
    expect(screen.getByDisplayValue("新規案件")).toBeInTheDocument();
    expect(screen.getByDisplayValue("AIが提案した説明")).toBeInTheDocument();
  });

  it("calls onApprove with edited name and description", () => {
    const onApprove = vi.fn();
    render(<NewProjectProposal {...defaultProps} onApprove={onApprove} />);
    const nameInput = screen.getByDisplayValue("新規案件");
    fireEvent.change(nameInput, { target: { value: "修正された案件名" } });
    fireEvent.click(screen.getByText("案件を作成"));
    expect(onApprove).toHaveBeenCalledWith("mail-1", "修正された案件名", "AIが提案した説明", undefined);
  });

  it("calls onReject with mailId", () => {
    const onReject = vi.fn();
    render(<NewProjectProposal {...defaultProps} onReject={onReject} />);
    fireEvent.click(screen.getByText("却下"));
    expect(onReject).toHaveBeenCalledWith("mail-1");
  });

  it("disables approve button when name is empty", () => {
    render(<NewProjectProposal {...defaultProps} />);
    const nameInput = screen.getByDisplayValue("新規案件");
    fireEvent.change(nameInput, { target: { value: "" } });
    const button = screen.getByText("案件を作成");
    expect(button).toBeDisabled();
  });

  it("disables approve button when name is whitespace only", () => {
    render(<NewProjectProposal {...defaultProps} />);
    const nameInput = screen.getByDisplayValue("新規案件");
    fireEvent.change(nameInput, { target: { value: "   " } });
    const button = screen.getByText("案件を作成");
    expect(button).toBeDisabled();
  });

  it("calls onApprove with undefined description when empty", () => {
    const onApprove = vi.fn();
    render(<NewProjectProposal {...defaultProps} suggestedDescription={undefined} onApprove={onApprove} />);
    fireEvent.click(screen.getByText("案件を作成"));
    expect(onApprove).toHaveBeenCalledWith("mail-1", "新規案件", undefined, undefined);
  });

  it("does not show a target path when no parent is proposed", () => {
    render(<NewProjectProposal {...defaultProps} />);
    expect(screen.queryByText(/^作成先:/)).not.toBeInTheDocument();
  });

  it("new project proposal shows target path when parent is proposed", () => {
    const projects = [p("tour", "ツアー", null)];
    render(
      <NewProjectProposal
        {...defaultProps}
        parentProjectId="tour"
        projects={projects}
      />,
    );
    expect(screen.getByText("作成先: ツアー")).toBeInTheDocument();
  });

  it("shows a parent selection dropdown including root, defaulting to the proposed parent", () => {
    const projects = [p("tour", "ツアー", null), p("other", "別案件", null)];
    render(
      <NewProjectProposal
        {...defaultProps}
        parentProjectId="tour"
        projects={projects}
      />,
    );
    const select = screen.getByLabelText("作成先の親案件");
    expect(select).toHaveValue("tour");
    expect(screen.getByRole("option", { name: "ルート（親なし）" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "別案件" })).toBeInTheDocument();
  });

  it("labels parent options with their full path so same-named projects under different parents are distinguishable", () => {
    const projects = [
      p("tour", "ツアー", null),
      p("a", "案件X", "tour"),
      p("other", "別案件", null),
      p("b", "案件X", "other"),
    ];
    render(
      <NewProjectProposal
        {...defaultProps}
        parentProjectId="tour"
        projects={projects}
      />,
    );
    const select = screen.getByLabelText("作成先の親案件");
    expect(
      screen.getByRole("option", { name: "ツアー > 案件X" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: "別案件 > 案件X" }),
    ).toBeInTheDocument();
    // ソートはパス文字列順
    const optionTexts = Array.from(select.querySelectorAll("option")).map(
      (o) => o.textContent,
    );
    expect(optionTexts).toEqual([
      "ルート（親なし）",
      "ツアー",
      "ツアー > 案件X",
      "別案件",
      "別案件 > 案件X",
    ]);
  });

  it("changing the parent dropdown updates the displayed target path and passed parentProjectId", () => {
    const onApprove = vi.fn();
    const projects = [p("tour", "ツアー", null), p("other", "別案件", null)];
    render(
      <NewProjectProposal
        {...defaultProps}
        parentProjectId="tour"
        projects={projects}
        onApprove={onApprove}
      />,
    );
    const select = screen.getByLabelText("作成先の親案件");
    fireEvent.change(select, { target: { value: "other" } });
    expect(screen.getByText("作成先: 別案件")).toBeInTheDocument();

    fireEvent.click(screen.getByText("案件を作成"));
    expect(onApprove).toHaveBeenCalledWith("mail-1", "新規案件", "AIが提案した説明", "other");
  });

  it("selecting root in the dropdown passes undefined as parentProjectId", () => {
    const onApprove = vi.fn();
    const projects = [p("tour", "ツアー", null)];
    render(
      <NewProjectProposal
        {...defaultProps}
        parentProjectId="tour"
        projects={projects}
        onApprove={onApprove}
      />,
    );
    const select = screen.getByLabelText("作成先の親案件");
    fireEvent.change(select, { target: { value: "" } });
    fireEvent.click(screen.getByText("案件を作成"));
    expect(onApprove).toHaveBeenCalledWith("mail-1", "新規案件", "AIが提案した説明", undefined);
  });
});
