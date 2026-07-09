import { describe, it, expect, vi, beforeEach } from "vitest";
import { useMailStore } from "../../stores/mailStore";
import { useAccountStore } from "../../stores/accountStore";
import { useUiStore } from "../../stores/uiStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

let syncProgressHandler: ((event: { payload: unknown }) => void) | null = null;
const mockUnlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    if (name === "sync-progress") syncProgressHandler = handler;
    return Promise.resolve(mockUnlisten);
  }),
}));

describe("mailStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMailStore.setState({
      threads: [],
      selectedThread: null,
      selectedMail: null,
      syncing: false,
      needsReauth: false,
      unclassifiedMails: [],
      error: null,
      syncProgress: null,
    });
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useUiStore.setState({ viewMode: "threads" });
  });

  describe("fetchThreads", () => {
    it("sets threads on success", async () => {
      const threads = [
        { thread_id: "t1", subject: "Thread A", last_date: "2026-04-13", mail_count: 2, from_addrs: ["a@b.com"], mails: [] },
      ];
      mockInvoke.mockResolvedValue(threads);

      await useMailStore.getState().fetchThreads("acc1", "INBOX");

      expect(mockInvoke).toHaveBeenCalledWith("get_threads", { accountId: "acc1", folder: "INBOX" });
      expect(useMailStore.getState().threads).toEqual(threads);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("fetch error");

      await useMailStore.getState().fetchThreads("acc1", "INBOX");

      expect(useMailStore.getState().error).toBe("fetch error");
    });
  });

  describe("syncAccount", () => {
    it("sets syncing state and returns count", async () => {
      mockInvoke.mockResolvedValue(5);

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(5);
      expect(useMailStore.getState().syncing).toBe(false);
    });

    it("returns 0 and sets error on failure", async () => {
      mockInvoke.mockRejectedValue("sync error");

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(0);
      expect(useMailStore.getState().error).toBe("sync error");
      expect(useMailStore.getState().syncing).toBe(false);
    });

    it("sets needsReauth on reauth error", async () => {
      mockInvoke.mockRejectedValue("Reauth required: acc1");

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(0);
      expect(useMailStore.getState().needsReauth).toBe(true);
      expect(useMailStore.getState().syncing).toBe(false);
    });

    it("does not set needsReauth on other errors", async () => {
      mockInvoke.mockRejectedValue("IMAP connection failed");

      await useMailStore.getState().syncAccount("acc1");

      expect(useMailStore.getState().needsReauth).toBe(false);
    });
  });

  describe("selectThread", () => {
    it("sets selectedThread and clears selectedMail", () => {
      const thread = { thread_id: "t1", subject: "A", last_date: "", mail_count: 1, from_addrs: [], mails: [] };
      useMailStore.setState({ selectedMail: { id: "m1" } as never });

      useMailStore.getState().selectThread(thread);

      expect(useMailStore.getState().selectedThread).toEqual(thread);
      expect(useMailStore.getState().selectedMail).toBeNull();
    });

    it("clears selectedThread with null", () => {
      useMailStore.getState().selectThread(null);
      expect(useMailStore.getState().selectedThread).toBeNull();
    });
  });

  describe("selectMail", () => {
    it("sets selectedMail", () => {
      const mail = { id: "m1", subject: "Test" } as never;
      useMailStore.getState().selectMail(mail);
      expect(useMailStore.getState().selectedMail).toEqual(mail);
    });
  });

  describe("fetchUnclassified", () => {
    it("sets unclassifiedMails on success", async () => {
      const mails = [{ id: "m1", subject: "Test" }];
      mockInvoke.mockResolvedValue(mails);

      await useMailStore.getState().fetchUnclassified("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_mails", { accountId: "acc1" });
      expect(useMailStore.getState().unclassifiedMails).toEqual(mails);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("fetch error");

      await useMailStore.getState().fetchUnclassified("acc1");

      expect(useMailStore.getState().error).toBe("fetch error");
    });
  });

  describe("moveMail", () => {
    it("calls move_mail and removes mail from unclassified", async () => {
      useMailStore.setState({
        unclassifiedMails: [
          { id: "m1" } as never,
          { id: "m2" } as never,
        ],
      });
      mockInvoke.mockResolvedValueOnce(undefined); // move_mail

      await useMailStore.getState().moveMail("m1", "proj1");

      expect(mockInvoke).toHaveBeenCalledWith("move_mail", { mailId: "m1", projectId: "proj1" });
      expect(useMailStore.getState().unclassifiedMails).toHaveLength(1);
      expect(useMailStore.getState().unclassifiedMails[0].id).toBe("m2");
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("move error");

      await useMailStore.getState().moveMail("m1", "proj1");

      expect(useMailStore.getState().error).toBe("move error");
    });
  });

  describe("removeUnclassifiedMail", () => {
    it("removes a specific mail from unclassifiedMails", () => {
      useMailStore.setState({
        unclassifiedMails: [
          { id: "m1" } as never,
          { id: "m2" } as never,
        ],
      });

      useMailStore.getState().removeUnclassifiedMail("m1");

      expect(useMailStore.getState().unclassifiedMails).toHaveLength(1);
      expect(useMailStore.getState().unclassifiedMails[0].id).toBe("m2");
    });
  });

  describe("sync progress", () => {
    it("updates syncProgress on sync-progress events", async () => {
      await useMailStore.getState().initSyncListener();
      syncProgressHandler!({
        payload: { account_id: "acc1", done: 100, total: 5000 },
      });
      expect(useMailStore.getState().syncProgress).toEqual({
        account_id: "acc1",
        done: 100,
        total: 5000,
      });
    });

    it("refreshes lists every 500 mails and at completion, not on every batch", async () => {
      mockInvoke.mockResolvedValue([]);
      await useMailStore.getState().initSyncListener();

      syncProgressHandler!({ payload: { account_id: "acc1", done: 100, total: 1200 } });
      expect(mockInvoke).not.toHaveBeenCalledWith("get_threads", expect.anything());

      syncProgressHandler!({ payload: { account_id: "acc1", done: 500, total: 1200 } });
      expect(mockInvoke).toHaveBeenCalledWith("get_threads", {
        accountId: "acc1",
        folder: "INBOX",
      });
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_mails", {
        accountId: "acc1",
      });

      mockInvoke.mockClear();
      mockInvoke.mockResolvedValue([]);
      syncProgressHandler!({ payload: { account_id: "acc1", done: 1200, total: 1200 } });
      expect(mockInvoke).toHaveBeenCalledWith("get_threads", {
        accountId: "acc1",
        folder: "INBOX",
      });
    });

    it("refreshes threads and unclassified when viewMode is 'threads' and the synced account is selected", async () => {
      useAccountStore.setState({ selectedAccountId: "acc1" });
      useUiStore.setState({ viewMode: "threads" });
      mockInvoke.mockResolvedValue([]);
      await useMailStore.getState().initSyncListener();

      syncProgressHandler!({ payload: { account_id: "acc1", done: 500, total: 1200 } });

      expect(mockInvoke).toHaveBeenCalledWith("get_threads", {
        accountId: "acc1",
        folder: "INBOX",
      });
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_mails", {
        accountId: "acc1",
      });
    });

    it("does not call get_threads when viewMode is 'project' (but still refreshes unclassified)", async () => {
      useAccountStore.setState({ selectedAccountId: "acc1" });
      useUiStore.setState({ viewMode: "project" });
      mockInvoke.mockResolvedValue([]);
      await useMailStore.getState().initSyncListener();

      syncProgressHandler!({ payload: { account_id: "acc1", done: 500, total: 1200 } });

      expect(mockInvoke).not.toHaveBeenCalledWith("get_threads", expect.anything());
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_mails", {
        accountId: "acc1",
      });
    });

    it("does not refresh anything when a different account is selected", async () => {
      useAccountStore.setState({ selectedAccountId: "acc2" });
      useUiStore.setState({ viewMode: "threads" });
      mockInvoke.mockResolvedValue([]);
      await useMailStore.getState().initSyncListener();

      syncProgressHandler!({ payload: { account_id: "acc1", done: 500, total: 1200 } });

      expect(mockInvoke).not.toHaveBeenCalledWith("get_threads", expect.anything());
      expect(mockInvoke).not.toHaveBeenCalledWith("get_unclassified_mails", expect.anything());
    });

    it("clears syncProgress when syncAccount finishes", async () => {
      mockInvoke.mockResolvedValue(3);
      useMailStore.setState({
        syncProgress: { account_id: "acc1", done: 100, total: 200 },
      });
      await useMailStore.getState().syncAccount("acc1");
      expect(useMailStore.getState().syncProgress).toBeNull();
    });

    it("does not start another sync while one is in flight", async () => {
      useMailStore.setState({ syncing: true });
      const count = await useMailStore.getState().syncAccount("acc1");
      expect(count).toBe(0);
      expect(mockInvoke).not.toHaveBeenCalledWith("sync_account", expect.anything());
      expect(useMailStore.getState().syncing).toBe(true);
    });
  });
});
