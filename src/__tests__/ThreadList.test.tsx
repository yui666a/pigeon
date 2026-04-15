import { render, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const mockAccountStore = {
  selectedAccountId: "acc1",
  startReauth: vi.fn(),
};

const mockMailStore = {
  threads: [] as import("../types/mail").Thread[],
  syncing: false,
  needsReauthAccountId: null as string | null,
  selectedThread: null as import("../types/mail").Thread | null,
  fetchThreads: vi.fn(async () => {}),
  syncAccount: vi.fn(async () => 0),
  selectThread: vi.fn(),
  setThreads: vi.fn(),
};

const mockProjectStore = {
  selectedProjectId: null as string | null,
};

vi.mock("../stores/accountStore", () => ({
  useAccountStore: (selector: (s: typeof mockAccountStore) => unknown) =>
    selector(mockAccountStore),
}));

vi.mock("../stores/mailStore", () => ({
  useMailStore: (selector?: (s: typeof mockMailStore) => unknown) =>
    selector ? selector(mockMailStore) : mockMailStore,
}));

vi.mock("../stores/projectStore", () => ({
  useProjectStore: (selector: (s: typeof mockProjectStore) => unknown) =>
    selector(mockProjectStore),
}));

describe("ThreadList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockAccountStore.selectedAccountId = "acc1";
    mockMailStore.needsReauthAccountId = null;
  });

  it("does not sync while selected account needs reauth", async () => {
    mockMailStore.needsReauthAccountId = "acc1";

    render(<ThreadList viewMode="threads" />);

    await waitFor(() => {
      expect(mockMailStore.syncAccount).not.toHaveBeenCalled();
    });
  });

  it("re-syncs once selected account reauth is cleared", async () => {
    mockMailStore.needsReauthAccountId = "acc1";

    const { rerender } = render(<ThreadList viewMode="threads" />);
    expect(mockMailStore.syncAccount).not.toHaveBeenCalled();

    mockMailStore.needsReauthAccountId = null;
    rerender(<ThreadList viewMode="threads" />);

    await waitFor(() => {
      expect(mockMailStore.syncAccount).toHaveBeenCalledWith("acc1");
      expect(mockMailStore.fetchThreads).toHaveBeenCalledWith("acc1", "INBOX");
    });
  });
});
