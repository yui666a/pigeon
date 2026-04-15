import { describe, it, expect, vi, beforeEach } from "vitest";
import { useMailStore } from "../../stores/mailStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("mailStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMailStore.setState({
      threads: [],
      selectedThread: null,
      selectedMail: null,
      syncing: false,
      needsReauthAccountId: null,
      unclassifiedMails: [],
      error: null,
    });
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
      mockInvoke.mockResolvedValue({ count: 5, reauth_required: false });

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

    it("sets needsReauthAccountId on reauth response", async () => {
      mockInvoke.mockResolvedValue({ count: 0, reauth_required: true });

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(0);
      expect(useMailStore.getState().needsReauthAccountId).toBe("acc1");
      expect(useMailStore.getState().syncing).toBe(false);
    });

    it("does not set needsReauthAccountId on other errors", async () => {
      mockInvoke.mockRejectedValue("IMAP connection failed");

      await useMailStore.getState().syncAccount("acc1");

      expect(useMailStore.getState().needsReauthAccountId).toBeNull();
    });

    it("keeps other account reauth flag on non-reauth error", async () => {
      useMailStore.setState({ needsReauthAccountId: "acc2" });
      mockInvoke.mockRejectedValue("IMAP connection failed");

      await useMailStore.getState().syncAccount("acc1");

      expect(useMailStore.getState().needsReauthAccountId).toBe("acc2");
    });

    it("clears matching needsReauthAccountId before retry sync", async () => {
      useMailStore.setState({ needsReauthAccountId: "acc1" });
      mockInvoke.mockResolvedValue({ count: 3, reauth_required: false });

      await useMailStore.getState().syncAccount("acc1");

      expect(useMailStore.getState().needsReauthAccountId).toBeNull();
    });
  });

  describe("clearNeedsReauth", () => {
    it("clears state when accountId matches", () => {
      useMailStore.setState({ needsReauthAccountId: "acc1" });
      useMailStore.getState().clearNeedsReauth("acc1");
      expect(useMailStore.getState().needsReauthAccountId).toBeNull();
    });

    it("keeps state when accountId does not match", () => {
      useMailStore.setState({ needsReauthAccountId: "acc1" });
      useMailStore.getState().clearNeedsReauth("acc2");
      expect(useMailStore.getState().needsReauthAccountId).toBe("acc1");
    });

    it("clears state when called without accountId", () => {
      useMailStore.setState({ needsReauthAccountId: "acc1" });
      useMailStore.getState().clearNeedsReauth();
      expect(useMailStore.getState().needsReauthAccountId).toBeNull();
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
});
