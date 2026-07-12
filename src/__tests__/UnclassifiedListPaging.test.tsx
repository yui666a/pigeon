import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { UnclassifiedList } from "../components/thread-list/UnclassifiedList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useClassifyStore } from "../stores/classifyStore";
import { useProjectStore } from "../stores/projectStore";
import type { Mail } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const mail = (i: number): Mail => ({
  id: `m${i}`,
  account_id: "acc1",
  folder: "INBOX",
  message_id: `<m${i}@example.com>`,
  in_reply_to: null,
  references: null,
  from_addr: "a@example.com",
  to_addr: "b@example.com",
  cc_addr: null,
  subject: `未分類 ${i}`,
  body_text: "本文",
  body_html: null,
  date: "2026-07-09T00:00:00Z",
  has_attachments: false,
  raw_size: 1024,
  uid: i,
  flags: null,
  is_read: false,
  fetched_at: "2026-07-09T00:00:00Z",
});

describe("UnclassifiedList paging", () => {
  beforeEach(() => {
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useMailStore.setState({
      unclassifiedMails: Array.from({ length: 250 }, (_, i) => mail(i)),
      threads: [],
      syncing: false,
      needsReauth: false,
      selectedThread: null,
      selectedMail: null,
      syncProgress: null,
    });
    useClassifyStore.setState({
      pendingProposal: null,
      classifying: false,
    });
    useProjectStore.setState({
      projects: [],
    });
  });

  it("renders only the first 200 unclassified mails with a show-more button", () => {
    render(<UnclassifiedList />);
    expect(screen.getByText("未分類 0")).toBeInTheDocument();
    expect(screen.getByText("未分類 199")).toBeInTheDocument();
    expect(screen.queryByText("未分類 200")).not.toBeInTheDocument();
    expect(screen.getByText(/もっと見る（残り 50 件）/)).toBeInTheDocument();
  });

  it("reveals all mails on click", () => {
    render(<UnclassifiedList />);
    fireEvent.click(screen.getByText(/もっと見る/));
    expect(screen.getByText("未分類 249")).toBeInTheDocument();
    expect(screen.queryByText(/もっと見る/)).not.toBeInTheDocument();
  });
});
