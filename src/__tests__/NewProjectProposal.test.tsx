import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { NewProjectProposal } from "../components/common/NewProjectProposal";

describe("NewProjectProposal", () => {
  const defaultProps = {
    mailId: "mail-1",
    suggestedName: "新規案件",
    suggestedDescription: "AIが提案した説明",
    reason: "既存プロジェクトに一致なし",
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
    expect(onApprove).toHaveBeenCalledWith("mail-1", "修正された案件名", "AIが提案した説明");
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
    expect(onApprove).toHaveBeenCalledWith("mail-1", "新規案件", undefined);
  });
});
