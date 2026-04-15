import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ErrorToast } from "../components/common/ErrorToast";
import { useErrorStore } from "../stores/errorStore";

describe("ErrorToast", () => {
  beforeEach(() => {
    useErrorStore.setState({ errors: [] });
  });

  it("renders nothing when there are no errors", () => {
    const { container } = render(<ErrorToast />);
    expect(container.firstChild).toBeNull();
  });

  it("renders error messages when they exist", () => {
    useErrorStore.setState({
      errors: [
        { id: "1", message: "Network error", timestamp: Date.now() },
        { id: "2", message: "Server error", timestamp: Date.now() },
      ],
    });

    render(<ErrorToast />);

    expect(screen.getByText("Network error")).toBeInTheDocument();
    expect(screen.getByText("Server error")).toBeInTheDocument();
  });

  it("clicking the dismiss button removes the error", () => {
    const dismissError = vi.fn();

    useErrorStore.setState({
      errors: [{ id: "1", message: "Test error", timestamp: Date.now() }],
      dismissError,
    });

    render(<ErrorToast />);

    const dismissButton = screen.getByRole("button");
    fireEvent.click(dismissButton);

    expect(dismissError).toHaveBeenCalledWith("1");
  });

  it("renders multiple dismiss buttons for multiple errors", () => {
    useErrorStore.setState({
      errors: [
        { id: "1", message: "Error 1", timestamp: Date.now() },
        { id: "2", message: "Error 2", timestamp: Date.now() },
      ],
    });

    render(<ErrorToast />);

    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(2);
  });
});
