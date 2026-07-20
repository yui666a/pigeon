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
    expect(callsOf("get_threads_by_project")[0][1]).toEqual({
      projectId: "p1",
      limit: 200,
      offset: 0,
    });
    expect(callsOf("sync_account")).toHaveLength(0);
  });

  it("syncs then fetches inbox threads in threads view", async () => {
    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    expect(callsOf("sync_account")).toHaveLength(1);
    expect(callsOf("get_threads")).toHaveLength(1);
    expect(callsOf("get_threads")[0][1]).toEqual({
      accountId: "acc1",
      folder: "INBOX",
      limit: 200,
      offset: 0,
    });
  });

  it("skips fetching threads when sync requires reauth", async () => {
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "sync_account") {
        return Promise.reject("Reauth required: acc1");
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    expect(useMailStore.getState().needsReauth).toBe(true);
    expect(callsOf("get_threads")).toHaveLength(0);
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
    await act(async () => {
      resolveFirstSync(0);
    });

    const staleCalls = callsOf("get_threads").filter(
      (c) => (c[1] as { accountId: string }).accountId === "acc1",
    );
    expect(staleCalls).toHaveLength(0);
  });
});
