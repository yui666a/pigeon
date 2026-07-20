import { render, screen, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useProjectStore } from "../stores/projectStore";
import { useSelectionStore } from "../stores/selectionStore";
import type { Mail, Thread } from "../types/mail";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const callsOf = (command: string) =>
  mockInvoke.mock.calls.filter((c) => c[0] === command);

function mail(id: string, subject: string): Mail {
  return {
    id,
    account_id: "acc1",
    folder: "INBOX",
    message_id: `<${id}@example.com>`,
    in_reply_to: null,
    references: null,
    from_addr: "sender@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject,
    body_text: "body",
    body_html: null,
    date: "2026-07-20T00:00:00Z",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    is_flagged: false,
    fetched_at: "2026-07-20T00:00:00Z",
  };
}

function thread(id: string, subject: string): Thread {
  return {
    thread_id: `t-${id}`,
    subject,
    last_date: "2026-07-20T00:00:00Z",
    mail_count: 1,
    from_addrs: ["sender@example.com"],
    mails: [mail(id, subject)],
    projects: [],
  };
}

describe("ThreadList renders cache before sync completes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useProjectStore.setState({ selectedProjectId: null, projects: [] });
    useMailStore.setState({
      threads: [],
      selectedThread: null,
      selectedMail: null,
      syncing: false,
      needsReauth: false,
      syncProgress: null,
    });
    useSelectionStore.getState().clear();
  });

  it("renders locally cached threads while the sync is still pending", async () => {
    let resolveSync!: (count: number) => void;
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") {
        return new Promise<number>((resolve) => {
          resolveSync = resolve;
        });
      }
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      if (command === "get_threads") {
        return Promise.resolve([thread("m1", "キャッシュ済みの件名")]);
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    // 同期は未解決のまま。ローカルキャッシュの取得だけが解決する
    await act(async () => {});

    expect(screen.getByText("キャッシュ済みの件名")).toBeInTheDocument();
    expect(callsOf("get_threads")).toHaveLength(1);
    // 同期はキャッシュ描画をブロックせず、裏で走っている
    expect(callsOf("sync_account")).toHaveLength(1);
    expect(useMailStore.getState().syncing).toBe(true);

    await act(async () => {
      resolveSync(0);
    });
  });

  it("refreshes the list once the background sync completes", async () => {
    let resolveSync!: (count: number) => void;
    let getThreadsCall = 0;
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") {
        return new Promise<number>((resolve) => {
          resolveSync = resolve;
        });
      }
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      if (command === "get_threads") {
        getThreadsCall += 1;
        return Promise.resolve(
          getThreadsCall === 1
            ? [thread("m1", "キャッシュ済みの件名")]
            : [thread("m1", "キャッシュ済みの件名"), thread("m2", "同期後の新着")],
        );
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {});
    expect(screen.queryByText("同期後の新着")).not.toBeInTheDocument();

    await act(async () => {
      resolveSync(1);
    });

    expect(screen.getByText("同期後の新着")).toBeInTheDocument();
    expect(callsOf("get_threads")).toHaveLength(2);
  });

  it("keeps the selected thread across the post-sync refresh", async () => {
    let resolveSync!: (count: number) => void;
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") {
        return new Promise<number>((resolve) => {
          resolveSync = resolve;
        });
      }
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      if (command === "get_threads") {
        return Promise.resolve([thread("m1", "キャッシュ済みの件名")]);
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    const cached = useMailStore.getState().threads[0];
    act(() => {
      useMailStore.setState({ selectedThread: cached });
    });

    await act(async () => {
      resolveSync(0);
    });

    // 同期後の再取得で選択が失われない
    expect(useMailStore.getState().selectedThread?.thread_id).toBe(
      cached.thread_id,
    );
  });

  it("still skips the post-sync refresh when reauth is required", async () => {
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") {
        return Promise.reject("Reauth required: acc1");
      }
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    expect(useMailStore.getState().needsReauth).toBe(true);
    // 初回のキャッシュ描画は行うが、同期失敗後の再取得はしない
    expect(callsOf("get_threads")).toHaveLength(1);
  });

  it("does not apply a stale post-sync refresh after switching accounts", async () => {
    let resolveFirstSync!: (count: number) => void;
    mockInvoke.mockImplementation((command: unknown, args: unknown) => {
      if (command === "sync_account") {
        const { accountId } = args as { accountId: string };
        if (accountId === "acc1") {
          return new Promise<number>((resolve) => {
            resolveFirstSync = resolve;
          });
        }
        return Promise.resolve(0);
      }
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {
      useAccountStore.setState({ selectedAccountId: "acc2" });
    });
    const before = callsOf("get_threads").filter(
      (c) => (c[1] as { accountId: string }).accountId === "acc1",
    ).length;

    await act(async () => {
      resolveFirstSync(0);
    });

    const after = callsOf("get_threads").filter(
      (c) => (c[1] as { accountId: string }).accountId === "acc1",
    ).length;
    // 切替後に acc1 の再取得が増えない（古い結果で上書きしない）
    expect(after).toBe(before);
  });
});
