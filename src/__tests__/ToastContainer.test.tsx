import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ToastContainer } from "../components/common/ToastContainer";
import { useErrorStore } from "../stores/errorStore";

describe("ToastContainer", () => {
  beforeEach(() => {
    useErrorStore.setState({ toasts: [] });
  });

  it("renders nothing when there are no toasts", () => {
    const { container } = render(<ToastContainer />);
    expect(container.firstChild).toBeNull();
  });

  it("renders toast messages when they exist", () => {
    useErrorStore.setState({
      toasts: [
        { id: "1", kind: "error", message: "Network error", timestamp: Date.now() },
        { id: "2", kind: "success", message: "メールを送信しました", timestamp: Date.now() },
      ],
    });

    render(<ToastContainer />);

    expect(screen.getByText("Network error")).toBeInTheDocument();
    expect(screen.getByText("メールを送信しました")).toBeInTheDocument();
  });

  it("styles error toasts red and success toasts green", () => {
    useErrorStore.setState({
      toasts: [
        { id: "1", kind: "error", message: "Network error", timestamp: Date.now() },
        { id: "2", kind: "success", message: "アーカイブしました", timestamp: Date.now() },
      ],
    });

    render(<ToastContainer />);

    expect(screen.getByText("Network error").closest("div")).toHaveClass(
      "bg-red-600",
    );
    expect(screen.getByText("アーカイブしました").closest("div")).toHaveClass(
      "bg-green-600",
    );
  });

  it("clicking the dismiss button removes the toast", () => {
    const dismissToast = vi.fn();

    useErrorStore.setState({
      toasts: [{ id: "1", kind: "error", message: "Test error", timestamp: Date.now() }],
      dismissToast,
    });

    render(<ToastContainer />);

    const dismissButton = screen.getByRole("button");
    fireEvent.click(dismissButton);

    expect(dismissToast).toHaveBeenCalledWith("1");
  });

  it("renders multiple dismiss buttons for multiple toasts", () => {
    useErrorStore.setState({
      toasts: [
        { id: "1", kind: "error", message: "Error 1", timestamp: Date.now() },
        { id: "2", kind: "success", message: "Success 1", timestamp: Date.now() },
      ],
    });

    render(<ToastContainer />);

    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(2);
  });
});
