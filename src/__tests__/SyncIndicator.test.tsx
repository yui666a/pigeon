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
    useMailStore.setState({ syncProgress: null, backfillProgress: null });
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

  it("shows backfill progress while backfilling", () => {
    useMailStore.setState({
      backfillProgress: { account_id: "acc1", done: 300, total: 5000 },
    });
    render(<SyncIndicator />);
    expect(
      screen.getByText(/過去メール取得中… 300 \/ 5,000/),
    ).toBeInTheDocument();
  });

  it("shows both rows when sync and backfill are somehow both in progress", () => {
    useMailStore.setState({
      syncProgress: { account_id: "acc1", done: 10, total: 20 },
      backfillProgress: { account_id: "acc2", done: 300, total: 5000 },
    });
    render(<SyncIndicator />);
    expect(screen.getByText(/メール同期中… 10 \/ 20/)).toBeInTheDocument();
    expect(
      screen.getByText(/過去メール取得中… 300 \/ 5,000/),
    ).toBeInTheDocument();
  });
});
