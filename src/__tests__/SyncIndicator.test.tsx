import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SyncIndicator } from "../components/sidebar/SyncIndicator";
import { useMailStore } from "../stores/mailStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("SyncIndicator", () => {
  beforeEach(() => {
    useMailStore.setState({ syncProgress: null });
  });

  it("renders nothing when no sync is in progress", () => {
    const { container } = render(<SyncIndicator />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows progress with thousands separators while syncing", () => {
    useMailStore.setState({
      syncProgress: { account_id: "acc1", done: 1200, total: 5000 },
    });
    render(<SyncIndicator />);
    expect(screen.getByText(/メール同期中… 1,200 \/ 5,000/)).toBeInTheDocument();
  });
});
