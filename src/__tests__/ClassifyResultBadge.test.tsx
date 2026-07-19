import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ClassifyResultBadge } from "../components/common/ClassifyResultBadge";

describe("ClassifyResultBadge", () => {
  it("returns null for user-assigned mails", () => {
    const { container } = render(
      <ClassifyResultBadge confidence={0.95} assignedBy="user" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows green AI badge for high confidence (>= 0.7)", () => {
    render(<ClassifyResultBadge confidence={0.85} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("bg-green-100");
  });

  it("shows yellow warning AI badge for uncertain confidence (0.4-0.7)", () => {
    render(<ClassifyResultBadge confidence={0.55} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("bg-yellow-100");
  });

  it("returns null for low confidence (< 0.4)", () => {
    const { container } = render(
      <ClassifyResultBadge confidence={0.2} assignedBy="ai" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows green badge at exactly 0.7 boundary", () => {
    render(<ClassifyResultBadge confidence={0.7} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge.className).toContain("bg-green-100");
  });

  it("shows yellow badge at exactly 0.4 boundary", () => {
    render(<ClassifyResultBadge confidence={0.4} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge.className).toContain("bg-yellow-100");
  });
});

describe("ClassifyResultBadge の操作", () => {
  it("onClick を渡すとボタンになり、クリックで通知する", () => {
    const onClick = vi.fn();
    render(
      <ClassifyResultBadge confidence={0.55} assignedBy="ai" onClick={onClick} />,
    );

    const button = screen.getByRole("button", { name: "AI分類を確認" });
    fireEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("onClick を渡さなければボタンにしない", () => {
    render(<ClassifyResultBadge confidence={0.55} assignedBy="ai" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
