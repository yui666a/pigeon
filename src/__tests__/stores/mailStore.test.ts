import { describe, it, expect, vi, beforeEach } from "vitest";
import { useMailStore } from "../../stores/mailStore";
import { useAccountStore } from "../../stores/accountStore";
import { useUiStore } from "../../stores/uiStore";
import { useErrorStore } from "../../stores/errorStore";
import type { Mail, Thread } from "../../types/mail";

function makeMail(id: string, overrides: Partial<Mail> = {}): Mail {
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
    subject: "Subject",
    body_text: "body",
    body_html: null,
    date: "2026-07-12T10:00:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    fetched_at: "2026-07-12T00:00:00",
    ...overrides,
  };
}

function makeThread(mails: Mail[]): Thread {
  return {
    thread_id: mails[0]?.message_id ?? "t1",
    subject: mails[0]?.subject ?? "Subject",
    last_date: mails[mails.length - 1]?.date ?? "",
    mail_count: mails.length,
    from_addrs: [],
    mails,
  };
}

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const mockNotifyNewMail = vi.fn();
vi.mock("../../utils/notifyNewMail", () => ({
  notifyNewMail: (...args: unknown[]) => mockNotifyNewMail(...args),
}));

let syncProgressHandler: ((event: { payload: unknown }) => void) | null = null;
let newMailHandler: ((event: { payload: unknown }) => void) | null = null;
const mockUnlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    if (name === "sync-progress") syncProgressHandler = handler;
    if (name === "new-mail-detected") newMailHandler = handler;
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
      unclassifiedThreads: [],
      error: null,
      syncProgress: null,
      unreadCounts: { by_project: {}, unclassified: 0 },
    });
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useUiStore.setState({ viewMode: "threads" });
    useErrorStore.setState({ toasts: [] });
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

    it("refreshes unread counts after successful sync", async () => {
      mockInvoke.mockImplementation((cmd: unknown) =>
        cmd === "sync_account"
          ? Promise.resolve(5)
          : Promise.resolve({ by_project: {}, unclassified: 0 }),
      );

      await useMailStore.getState().syncAccount("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_unread_counts", {
        accountId: "acc1",
      });
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
    it("sets selectedMail without marking when already read", () => {
      const mail = makeMail("m1", { is_read: true });
      useMailStore.getState().selectMail(mail);
      expect(useMailStore.getState().selectedMail).toEqual(mail);
      expect(mockInvoke).not.toHaveBeenCalledWith("mark_read", expect.anything());
    });

    it("marks unread mail as read and invokes mark_read", () => {
      mockInvoke.mockResolvedValue(undefined);
      const mail = makeMail("m1");
      useMailStore.setState({ unclassifiedMails: [mail] });

      useMailStore.getState().selectMail(mail);

      expect(mockInvoke).toHaveBeenCalledWith("mark_read", {
        accountId: "acc1",
        mailId: "m1",
      });
      expect(useMailStore.getState().selectedMail?.is_read).toBe(true);
      expect(useMailStore.getState().unclassifiedMails[0].is_read).toBe(true);
    });

    it("updates is_read inside threads too", () => {
      mockInvoke.mockResolvedValue(undefined);
      const mail = makeMail("m1");
      const thread = makeThread([mail]);
      useMailStore.setState({ threads: [thread], selectedThread: thread });

      useMailStore.getState().selectMail(mail);

      expect(useMailStore.getState().threads[0].mails[0].is_read).toBe(true);
      expect(useMailStore.getState().selectedThread?.mails[0].is_read).toBe(true);
    });

    it("keeps local read state even when mark_read fails", async () => {
      mockInvoke.mockRejectedValue("imap down");
      const mail = makeMail("m1");

      useMailStore.getState().selectMail(mail);
      await Promise.resolve();

      expect(useMailStore.getState().selectedMail?.is_read).toBe(true);
    });
  });

  describe("selectThread marks displayed mail as read", () => {
    it("marks the last mail of the thread (the displayed one)", () => {
      mockInvoke.mockResolvedValue(undefined);
      const m1 = makeMail("m1", { is_read: true });
      const m2 = makeMail("m2");
      const thread = makeThread([m1, m2]);
      useMailStore.setState({ threads: [thread] });

      useMailStore.getState().selectThread(thread);

      expect(mockInvoke).toHaveBeenCalledWith("mark_read", {
        accountId: "acc1",
        mailId: "m2",
      });
      expect(useMailStore.getState().selectedThread?.mails[1].is_read).toBe(true);
      expect(useMailStore.getState().threads[0].mails[1].is_read).toBe(true);
    });

    it("does not invoke mark_read when displayed mail is already read", () => {
      const thread = makeThread([makeMail("m1", { is_read: true })]);
      useMailStore.getState().selectThread(thread);
      expect(mockInvoke).not.toHaveBeenCalledWith("mark_read", expect.anything());
    });

    it("handles empty thread and null safely", () => {
      useMailStore.getState().selectThread(makeThread([]));
      useMailStore.getState().selectThread(null);
      expect(mockInvoke).not.toHaveBeenCalledWith("mark_read", expect.anything());
    });
  });

  describe("fetchUnreadCounts", () => {
    it("stores unread counts on success", async () => {
      const counts = { by_project: { p1: 3 }, unclassified: 2 };
      mockInvoke.mockResolvedValue(counts);

      await useMailStore.getState().fetchUnreadCounts("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_unread_counts", {
        accountId: "acc1",
      });
      expect(useMailStore.getState().unreadCounts).toEqual(counts);
    });

    it("keeps previous counts on failure", async () => {
      useMailStore.setState({ unreadCounts: { by_project: { p1: 1 }, unclassified: 0 } });
      mockInvoke.mockRejectedValue("boom");

      await useMailStore.getState().fetchUnreadCounts("acc1");

      expect(useMailStore.getState().unreadCounts).toEqual({
        by_project: { p1: 1 },
        unclassified: 0,
      });
    });
  });

  describe("fetchUnclassified", () => {
    it("sets unclassifiedThreads and flattened unclassifiedMails on success", async () => {
      const m1 = { id: "m1", subject: "Re: Test" };
      const m2 = { id: "m2", subject: "Re: Test" };
      const threads = [
        { thread_id: "t1", subject: "Re: Test", last_date: "", mail_count: 2, from_addrs: [], mails: [m1, m2] },
      ];
      mockInvoke.mockResolvedValue(threads);

      await useMailStore.getState().fetchUnclassified("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_threads", { accountId: "acc1" });
      expect(useMailStore.getState().unclassifiedThreads).toEqual(threads);
      expect(useMailStore.getState().unclassifiedMails).toEqual([m1, m2]);
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

  describe("deleteMail", () => {
    it("invokes delete_mail and removes the mail from all state on success", async () => {
      const m1 = makeMail("m1");
      const m2 = makeMail("m2");
      const thread = makeThread([m1, m2]);
      useMailStore.setState({
        threads: [thread],
        selectedThread: thread,
        selectedMail: m1,
        unclassifiedMails: [m1],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().deleteMail(m1);

      expect(mockInvoke).toHaveBeenCalledWith("delete_mail", {
        accountId: "acc1",
        mailId: "m1",
      });
      const s = useMailStore.getState();
      expect(s.threads[0].mails.map((m) => m.id)).toEqual(["m2"]);
      expect(s.threads[0].mail_count).toBe(1);
      expect(s.selectedThread?.mails.map((m) => m.id)).toEqual(["m2"]);
      expect(s.selectedMail).toBeNull();
      expect(s.unclassifiedMails).toHaveLength(0);
    });

    it("shows a success toast on successful delete", async () => {
      const m1 = makeMail("m1");
      useMailStore.setState({ threads: [makeThread([m1])] });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().deleteMail(m1);

      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0]).toMatchObject({ kind: "success", message: "削除しました" });
    });

    it("removes the whole thread when it becomes empty", async () => {
      const m1 = makeMail("m1");
      const thread = makeThread([m1]);
      useMailStore.setState({ threads: [thread], selectedThread: thread });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().deleteMail(m1);

      const s = useMailStore.getState();
      expect(s.threads).toHaveLength(0);
      expect(s.selectedThread).toBeNull();
    });

    it("refreshes unread counts after success", async () => {
      mockInvoke.mockResolvedValue(undefined);
      const m1 = makeMail("m1");
      useMailStore.setState({ threads: [makeThread([m1])] });

      await useMailStore.getState().deleteMail(m1);

      expect(mockInvoke).toHaveBeenCalledWith("get_unread_counts", {
        accountId: "acc1",
      });
    });

    it("keeps local state unchanged when the server delete fails", async () => {
      const m1 = makeMail("m1");
      const thread = makeThread([m1]);
      useMailStore.setState({ threads: [thread], selectedMail: m1 });
      mockInvoke.mockRejectedValue("IMAP error: EXPUNGE failed");

      await useMailStore.getState().deleteMail(m1);

      const s = useMailStore.getState();
      expect(s.threads).toHaveLength(1);
      expect(s.threads[0].mails).toHaveLength(1);
      expect(s.selectedMail?.id).toBe("m1");
      expect(mockInvoke).not.toHaveBeenCalledWith(
        "get_unread_counts",
        expect.anything(),
      );
      // 失敗時は成功トーストを出さない（エラートーストのみ）
      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0].kind).toBe("error");
    });
  });

  describe("archiveMail", () => {
    it("invokes archive_mail and removes the mail from view state on success", async () => {
      const m1 = makeMail("m1");
      const m2 = makeMail("m2");
      const thread = makeThread([m1, m2]);
      useMailStore.setState({
        threads: [thread],
        selectedThread: thread,
        selectedMail: m1,
        unclassifiedMails: [m1],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().archiveMail(m1);

      expect(mockInvoke).toHaveBeenCalledWith("archive_mail", {
        accountId: "acc1",
        mailId: "m1",
      });
      const s = useMailStore.getState();
      expect(s.threads[0].mails.map((m) => m.id)).toEqual(["m2"]);
      expect(s.selectedMail).toBeNull();
      expect(s.unclassifiedMails).toHaveLength(0);
    });

    it("shows a success toast on successful archive", async () => {
      const m1 = makeMail("m1");
      useMailStore.setState({ threads: [makeThread([m1])] });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().archiveMail(m1);

      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0]).toMatchObject({
        kind: "success",
        message: "アーカイブしました",
      });
    });

    it("removes the whole thread when it becomes empty and refreshes unread counts", async () => {
      const m1 = makeMail("m1");
      const thread = makeThread([m1]);
      useMailStore.setState({ threads: [thread], selectedThread: thread });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().archiveMail(m1);

      expect(useMailStore.getState().threads).toHaveLength(0);
      expect(useMailStore.getState().selectedThread).toBeNull();
      expect(mockInvoke).toHaveBeenCalledWith("get_unread_counts", {
        accountId: "acc1",
      });
    });

    it("keeps local state unchanged when the server archive fails", async () => {
      const m1 = makeMail("m1");
      const thread = makeThread([m1]);
      useMailStore.setState({ threads: [thread], selectedThread: thread });
      mockInvoke.mockRejectedValue("IMAP error: UID COPY failed");

      await useMailStore.getState().archiveMail(m1);

      const s = useMailStore.getState();
      expect(s.threads).toHaveLength(1);
      expect(s.selectedThread?.mails).toHaveLength(1);
      // 失敗時は成功トーストを出さない（エラートーストのみ）
      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0].kind).toBe("error");
    });
  });

  describe("unarchiveMail", () => {
    it("invokes unarchive_mail and updates folder to INBOX in all state on success", async () => {
      const m1 = makeMail("m1", { folder: "Archive" });
      const m2 = makeMail("m2", { folder: "Archive" });
      const thread = makeThread([m1, m2]);
      useMailStore.setState({
        threads: [thread],
        selectedThread: thread,
        selectedMail: m1,
        unclassifiedMails: [m1],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useMailStore.getState().unarchiveMail(m1);

      expect(mockInvoke).toHaveBeenCalledWith("unarchive_mail", {
        accountId: "acc1",
        mailId: "m1",
      });
      const s = useMailStore.getState();
      // 除去ではなく folder のローカル更新（案件ビュー・検索に表示され続ける）
      expect(s.threads[0].mails.map((m) => m.folder)).toEqual([
        "INBOX",
        "Archive",
      ]);
      expect(s.selectedThread?.mails[0].folder).toBe("INBOX");
      expect(s.selectedMail?.folder).toBe("INBOX");
      expect(s.unclassifiedMails[0].folder).toBe("INBOX");
    });

    it("refreshes unread counts after success", async () => {
      mockInvoke.mockResolvedValue(undefined);
      const m1 = makeMail("m1", { folder: "Archive" });
      useMailStore.setState({ threads: [makeThread([m1])] });

      await useMailStore.getState().unarchiveMail(m1);

      expect(mockInvoke).toHaveBeenCalledWith("get_unread_counts", {
        accountId: "acc1",
      });
    });

    it("keeps local state unchanged when the backend fails", async () => {
      const m1 = makeMail("m1", { folder: "Archive" });
      const thread = makeThread([m1]);
      useMailStore.setState({ threads: [thread], selectedMail: m1 });
      mockInvoke.mockRejectedValue("Validation error: not archived");

      await useMailStore.getState().unarchiveMail(m1);

      const s = useMailStore.getState();
      expect(s.threads[0].mails[0].folder).toBe("Archive");
      expect(s.selectedMail?.folder).toBe("Archive");
      expect(mockInvoke).not.toHaveBeenCalledWith(
        "get_unread_counts",
        expect.anything(),
      );
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
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_threads", {
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
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_threads", {
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
      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_threads", {
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
      expect(mockInvoke).not.toHaveBeenCalledWith("get_unclassified_threads", expect.anything());
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

  describe("new mail detection (IMAP IDLE)", () => {
    it("syncs the detected account on new-mail-detected", async () => {
      mockInvoke.mockResolvedValue(0);
      await useMailStore.getState().initNewMailListener();

      newMailHandler!({ payload: { account_id: "acc2" } });

      // 表示中アカウント（acc1）と無関係に、検知されたアカウントを同期する
      expect(mockInvoke).toHaveBeenCalledWith("sync_account", {
        accountId: "acc2",
      });
    });

    it("does not start a sync while another sync is in flight", async () => {
      useMailStore.setState({ syncing: true });
      await useMailStore.getState().initNewMailListener();

      newMailHandler!({ payload: { account_id: "acc1" } });

      expect(mockInvoke).not.toHaveBeenCalledWith("sync_account", expect.anything());
    });

    it("returns an unlisten function", async () => {
      const unlisten = await useMailStore.getState().initNewMailListener();
      expect(unlisten).toBe(mockUnlisten);
    });

    it("notifies with the synced count when new mails were imported", async () => {
      mockInvoke.mockImplementation((cmd: unknown) =>
        cmd === "sync_account"
          ? Promise.resolve(4)
          : Promise.resolve({ by_project: {}, unclassified: 0 }),
      );
      await useMailStore.getState().initNewMailListener();

      newMailHandler!({ payload: { account_id: "acc1" } });

      await vi.waitFor(() => {
        expect(mockNotifyNewMail).toHaveBeenCalledWith(4, "acc1");
      });
    });

    it("does not notify when the sync imported nothing", async () => {
      mockInvoke.mockImplementation((cmd: unknown) =>
        cmd === "sync_account"
          ? Promise.resolve(0)
          : Promise.resolve({ by_project: {}, unclassified: 0 }),
      );
      await useMailStore.getState().initNewMailListener();

      newMailHandler!({ payload: { account_id: "acc1" } });

      // syncAccount(0件) の解決を待ってから未通知を検証する
      await vi.waitFor(() => {
        expect(mockInvoke).toHaveBeenCalledWith("sync_account", {
          accountId: "acc1",
        });
      });
      await Promise.resolve();
      expect(mockNotifyNewMail).not.toHaveBeenCalled();
    });

    it("does not notify when the sync was skipped by the in-flight guard", async () => {
      useMailStore.setState({ syncing: true });
      await useMailStore.getState().initNewMailListener();

      newMailHandler!({ payload: { account_id: "acc1" } });
      await Promise.resolve();
      await Promise.resolve();

      expect(mockNotifyNewMail).not.toHaveBeenCalled();
    });
  });
});
