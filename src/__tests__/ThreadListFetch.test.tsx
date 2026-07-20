import { render, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useProjectStore } from "../stores/projectStore";
import { useSelectionStore } from "../stores/selectionStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const callsOf = (command: string) =>
  mockInvoke.mock.calls.filter((c) => c[0] === command);

describe("ThreadList fetch triggers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") return Promise.resolve(0);
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      return Promise.resolve([]);
    });
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

  it("fetches project threads through the store in project view", async () => {
    useProjectStore.setState({ selectedProjectId: "p1" });

    render(<ThreadList viewMode="project" />);
    await act(async () => {});

    expect(callsOf("get_threads_by_project")).toHaveLength(1);
    expect(callsOf("get_threads_by_project")[0][1]).toEqual({ projectId: "p1" });
    expect(callsOf("sync_account")).toHaveLength(0);
  });

  it("fetches inbox threads from cache and syncs in threads view", async () => {
    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    expect(callsOf("sync_account")).toHaveLength(1);
    // キャッシュの先行取得と、同期完了後の再取得の2回
    expect(callsOf("get_threads")).toHaveLength(2);
    expect(callsOf("get_threads")[0][1]).toEqual({
      accountId: "acc1",
      folder: "INBOX",
    });
  });

  it("skips the post-sync refetch when sync requires reauth", async () => {
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") {
        return Promise.reject("Reauth required: acc1");
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    expect(useMailStore.getState().needsReauth).toBe(true);
    // 先行のキャッシュ取得のみ。再認証が必要なら同期後の再取得はしない
    expect(callsOf("get_threads")).toHaveLength(1);
  });

  it("does not fetch stale threads after switching accounts mid-sync", async () => {
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
    // acc1 の同期が終わる前にアカウントを切り替える（高速切替の競合）
    await act(async () => {
      useAccountStore.setState({ selectedAccountId: "acc2" });
    });
    const acc1CallsBefore = callsOf("get_threads").filter(
      (c) => (c[1] as { accountId: string }).accountId === "acc1",
    ).length;

    await act(async () => {
      resolveFirstSync(0);
    });

    const acc1CallsAfter = callsOf("get_threads").filter(
      (c) => (c[1] as { accountId: string }).accountId === "acc1",
    ).length;
    // 切替後に acc1 の同期が完了しても、古い結果で新しい一覧を上書きしない
    expect(acc1CallsAfter).toBe(acc1CallsBefore);
  });
});
