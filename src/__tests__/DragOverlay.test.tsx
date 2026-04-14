import { render, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { DragOverlay } from "../components/common/DragOverlay";
import { useDragStore } from "../stores/dragStore";

describe("DragOverlay", () => {
  beforeEach(() => {
    useDragStore.setState({
      draggingMailIds: null,
      mouseX: 0,
      mouseY: 0,
      dragLabel: "",
    });
  });

  it("renders nothing when not dragging", () => {
    const { container } = render(<DragOverlay />);
    expect(container.firstChild).toBeNull();
  });

  it("renders drag label when dragging", () => {
    useDragStore.setState({
      draggingMailIds: ["m1"],
      mouseX: 100,
      mouseY: 200,
      dragLabel: "テストメール",
    });

    render(<DragOverlay />);
    expect(screen.getByText("テストメール")).toBeInTheDocument();
  });

  it("shows count badge for multiple mails", () => {
    useDragStore.setState({
      draggingMailIds: ["m1", "m2", "m3"],
      mouseX: 100,
      mouseY: 200,
      dragLabel: "テストメール",
    });

    render(<DragOverlay />);
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("does not show count badge for single mail", () => {
    useDragStore.setState({
      draggingMailIds: ["m1"],
      mouseX: 100,
      mouseY: 200,
      dragLabel: "テストメール",
    });

    render(<DragOverlay />);
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it("positions at mouse coordinates with offset", () => {
    useDragStore.setState({
      draggingMailIds: ["m1"],
      mouseX: 150,
      mouseY: 250,
      dragLabel: "テスト",
    });

    const { container } = render(<DragOverlay />);
    const overlay = container.firstElementChild as HTMLElement;
    expect(overlay.style.top).toBe("262px");
    expect(overlay.style.left).toBe("162px");
  });
});
