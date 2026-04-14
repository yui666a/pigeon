import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ClassifyButton } from "../components/thread-list/ClassifyButton";
import { useClassifyStore } from "../stores/classifyStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

describe("ClassifyButton", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("shows classify button in idle state", () => {
    useClassifyStore.setState({ classifying: false, progress: null, classifyAll: vi.fn(), cancelClassification: vi.fn() });
    render(<ClassifyButton accountId="acc1" />);
    expect(screen.getByText("分類する")).toBeInTheDocument();
  });

  it("calls classifyAll when button is clicked", () => {
    const classifyAll = vi.fn();
    useClassifyStore.setState({ classifying: false, progress: null, classifyAll, cancelClassification: vi.fn() });
    render(<ClassifyButton accountId="acc1" />);
    fireEvent.click(screen.getByText("分類する"));
    expect(classifyAll).toHaveBeenCalledWith("acc1");
  });

  it("shows progress bar when classifying", () => {
    useClassifyStore.setState({ classifying: true, progress: { current: 3, total: 10 }, classifyAll: vi.fn(), cancelClassification: vi.fn() });
    render(<ClassifyButton accountId="acc1" />);
    expect(screen.getByText("3 / 10")).toBeInTheDocument();
    expect(screen.getByText("キャンセル")).toBeInTheDocument();
  });

  it("calls cancelClassification when cancel is clicked", () => {
    const cancelClassification = vi.fn();
    useClassifyStore.setState({ classifying: true, progress: { current: 1, total: 5 }, classifyAll: vi.fn(), cancelClassification });
    render(<ClassifyButton accountId="acc1" />);
    fireEvent.click(screen.getByText("キャンセル"));
    expect(cancelClassification).toHaveBeenCalledTimes(1);
  });

  it("shows progress bar without text when progress is null during classifying", () => {
    useClassifyStore.setState({ classifying: true, progress: null, classifyAll: vi.fn(), cancelClassification: vi.fn() });
    render(<ClassifyButton accountId="acc1" />);
    expect(screen.getByText("キャンセル")).toBeInTheDocument();
    expect(screen.queryByText("/")).not.toBeInTheDocument();
  });
});
