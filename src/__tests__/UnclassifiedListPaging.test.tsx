import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { UnclassifiedList } from "../components/thread-list/UnclassifiedList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useClassifyStore } from "../stores/classifyStore";
import { useProjectStore } from "../stores/projectStore";
import type { Mail, Thread } from "../types/mail";

const mockInvoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
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
  is_flagged: false,
  fetched_at: "2026-07-09T00:00:00Z",
});

const threadOf = (m: Mail): Thread => ({
  thread_id: m.message_id,
  subject: m.subject,
  last_date: m.date,
  mail_count: 1,
  from_addrs: [m.from_addr],
  mails: [m],
  projects: [],
});

/**
 * ページングはサーバ側。一覧は取得済みを全部描画し、後続の有無で
 * 「もっと見る」を出す（ADR 0006 決定5）
 */
describe("UnclassifiedList paging", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue([]);
    useAccountStore.setState({ selectedAccountId: "acc1" });
    const mails = Array.from({ length: 200 }, (_, i) => mail(i));
    useMailStore.setState({
      unclassifiedMails: mails,
      unclassifiedThreads: mails.map(threadOf),
      hasMoreUnclassified: true,
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

  it("取得済みを描画し、後続があれば「もっと見る」を出す", () => {
    render(<UnclassifiedList />);
    expect(screen.getByText("未分類 0")).toBeInTheDocument();
    expect(screen.getByText("未分類 199")).toBeInTheDocument();
    expect(screen.getByText(/もっと見る/)).toBeInTheDocument();
  });

  it("後続が無ければ「もっと見る」を出さない", () => {
    useMailStore.setState({ hasMoreUnclassified: false });
    render(<UnclassifiedList />);
    expect(screen.queryByText(/もっと見る/)).not.toBeInTheDocument();
  });

  it("クリックで次ページを offset 付きで取得し追記する", async () => {
    // 初回マウントの取得（offset 0）は先頭200件＋後続あり、
    // 「もっと見る」の取得（offset 200）は201件目を返す
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === "get_unclassified_threads") {
        const { offset } = args as { offset: number };
        return offset === 0
          ? Promise.resolve({
              threads: Array.from({ length: 200 }, (_, i) => threadOf(mail(i))),
              has_more: true,
            })
          : Promise.resolve({ threads: [threadOf(mail(200))], has_more: false });
      }
      return Promise.resolve([]);
    });

    render(<UnclassifiedList />);
    await waitFor(() => expect(screen.getByText(/もっと見る/)).toBeInTheDocument());
    mockInvoke.mockClear();

    fireEvent.click(screen.getByText(/もっと見る/));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_threads", {
        accountId: "acc1",
        limit: 200,
        offset: 200,
      });
    });
    await waitFor(() => expect(screen.getByText("未分類 200")).toBeInTheDocument());
    expect(screen.getByText("未分類 0")).toBeInTheDocument();
  });
});
