import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useProjectStore } from "../stores/projectStore";
import type { Thread } from "../types/mail";

const mockInvoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));
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

/**
 * ページングはサーバ側で行うため、一覧は「取得済みのスレッドを全部描画し、
 * 後続の有無（has_more）でボタンを出す」挙動になる（ADR 0006 決定5）。
 */
describe("ThreadList paging", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useProjectStore.setState({ selectedProjectId: "p1" });
    useMailStore.setState({
      threads: Array.from({ length: 200 }, (_, i) => thread(i)),
      hasMoreThreads: true,
      syncing: false,
      needsReauth: false,
      selectedThread: null,
      syncProgress: null,
    });
  });

  it("取得済みスレッドを描画し、後続があれば「もっと見る」を出す", () => {
    render(<ThreadList viewMode="project" />);
    expect(screen.getByText("件名 0")).toBeInTheDocument();
    expect(screen.getByText("件名 199")).toBeInTheDocument();
    expect(screen.getByText(/もっと見る/)).toBeInTheDocument();
  });

  it("後続が無ければ「もっと見る」を出さない", () => {
    useMailStore.setState({ hasMoreThreads: false });
    render(<ThreadList viewMode="project" />);
    expect(screen.queryByText(/もっと見る/)).not.toBeInTheDocument();
  });

  it("クリックで次ページを offset 付きで取得し、一覧へ追記する", async () => {
    // 初回マウントの取得（offset 0）は先頭200件＋後続あり、
    // 「もっと見る」の取得（offset 200）は201件目を返す
    mockInvoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === "get_threads_by_project") {
        const { offset } = args as { offset: number };
        return offset === 0
          ? Promise.resolve({
              threads: Array.from({ length: 200 }, (_, i) => thread(i)),
              has_more: true,
            })
          : Promise.resolve({ threads: [thread(200)], has_more: false });
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="project" />);
    await waitFor(() => expect(screen.getByText(/もっと見る/)).toBeInTheDocument());
    mockInvoke.mockClear();

    fireEvent.click(screen.getByText(/もっと見る/));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_threads_by_project", {
        projectId: "p1",
        limit: 200,
        offset: 200,
      });
    });
    await waitFor(() => expect(screen.getByText("件名 200")).toBeInTheDocument());
    // 追記なので既存のスレッドも残る
    expect(screen.getByText("件名 0")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByText(/もっと見る/)).not.toBeInTheDocument(),
    );
  });
});
