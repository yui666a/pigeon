import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
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
