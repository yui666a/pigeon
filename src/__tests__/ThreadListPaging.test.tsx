import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import type { Thread } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const thread = (i: number): Thread => ({
  thread_id: `t${i}`,
  subject: `件名 ${i}`,
  last_date: "2026-07-09T00:00:00Z",
  mail_count: 1,
  from_addrs: ["a@example.com"],
  mails: [],
  projects: [],
});

describe("ThreadList paging", () => {
  beforeEach(() => {
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useMailStore.setState({
      threads: Array.from({ length: 250 }, (_, i) => thread(i)),
      syncing: false,
      needsReauth: false,
      selectedThread: null,
      syncProgress: null,
    });
  });

  it("renders only the first 200 threads with a show-more button", () => {
    render(<ThreadList viewMode="project" />);
    expect(screen.getByText("件名 0")).toBeInTheDocument();
    expect(screen.getByText("件名 199")).toBeInTheDocument();
    expect(screen.queryByText("件名 200")).not.toBeInTheDocument();
    expect(screen.getByText(/もっと見る（残り 50 件）/)).toBeInTheDocument();
  });

  it("reveals more threads on click", () => {
    render(<ThreadList viewMode="project" />);
    fireEvent.click(screen.getByText(/もっと見る/));
    expect(screen.getByText("件名 249")).toBeInTheDocument();
    expect(screen.queryByText(/もっと見る/)).not.toBeInTheDocument();
  });
});
