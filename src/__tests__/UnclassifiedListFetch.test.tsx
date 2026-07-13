import { render, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { UnclassifiedList } from "../components/thread-list/UnclassifiedList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useClassifyStore } from "../stores/classifyStore";
import { useProjectStore } from "../stores/projectStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const fetchCalls = () =>
  mockInvoke.mock.calls.filter((c) => c[0] === "get_unclassified_threads");

describe("UnclassifiedList fetch triggers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue([]);
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useMailStore.setState({
      unclassifiedMails: [],
      unclassifiedThreads: [],
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
    useProjectStore.setState({ projects: [] });
  });

  it("fetches unclassified mails exactly once on mount", () => {
    render(<UnclassifiedList />);
    expect(fetchCalls()).toHaveLength(1);
  });

  it("does not fetch when classification starts", () => {
    render(<UnclassifiedList />);
    mockInvoke.mockClear();

    act(() => {
      useClassifyStore.setState({ classifying: true });
    });

    expect(fetchCalls()).toHaveLength(0);
  });

  it("re-fetches once when classification completes (true -> false)", () => {
    render(<UnclassifiedList />);
    act(() => {
      useClassifyStore.setState({ classifying: true });
    });
    mockInvoke.mockClear();

    act(() => {
      useClassifyStore.setState({ classifying: false });
    });

    expect(fetchCalls()).toHaveLength(1);
  });

  it("fetches again when the selected account changes", () => {
    render(<UnclassifiedList />);
    mockInvoke.mockClear();

    act(() => {
      useAccountStore.setState({ selectedAccountId: "acc2" });
    });

    expect(fetchCalls()).toHaveLength(1);
    expect(fetchCalls()[0][1]).toEqual({ accountId: "acc2" });
  });
});
